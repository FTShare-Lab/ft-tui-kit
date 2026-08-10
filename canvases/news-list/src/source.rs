use std::collections::BTreeSet;
use std::io::Read;
use std::thread;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};

use crate::config::RendererConfig;

const DEFAULT_MARKET_URL: &str = "https://market.ft.tech/gateway";
const ENDPOINT_PATH: &str = "api/v1/market/data/semantic-search-news";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTENT_CHARS: usize = 64 * 1024;
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct NewsDataset {
    pub items: Vec<NewsItem>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub news_id: String,
    pub source_site: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub article_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_time: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content: String,
    pub is_truncated: bool,
    pub is_reviewed: bool,
    pub score: f64,
}

#[derive(Debug, Deserialize)]
struct RawNewsItem {
    news_id: WireNewsId,
    #[serde(default)]
    source_site: Option<String>,
    #[serde(default)]
    article_url: Option<String>,
    #[serde(default)]
    publish_time: Option<String>,
    #[serde(default)]
    fetch_time: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    media_name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    is_truncated: Option<u8>,
    #[serde(default)]
    is_reviewed: Option<u8>,
    #[serde(default)]
    score: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireNewsId {
    Unsigned(u64),
    String(String),
}

pub fn search_news(config: &RendererConfig) -> Result<NewsDataset, String> {
    let base = std::env::var("FTSHARE_BASE_URL").unwrap_or_else(|_| DEFAULT_MARKET_URL.into());
    let endpoint = endpoint_url(&base)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(7))
        .user_agent("ft-financial-canvas/news-list")
        .build()
        .map_err(|error| format!("cannot create FTShare news client: {error}"))?;
    let params = query_params(config);

    for attempt in 0..MAX_ATTEMPTS {
        match client.get(endpoint.clone()).query(&params).send() {
            Ok(response) if response.status().is_success() => {
                return parse_response(response, config.limit, endpoint.as_str());
            }
            Ok(response) => {
                let status = response.status();
                if retryable_status(status) && attempt + 1 < MAX_ATTEMPTS {
                    thread::sleep(retry_delay(attempt));
                    continue;
                }
                return Err(response_error(response));
            }
            Err(error) if retryable_error(&error) && attempt + 1 < MAX_ATTEMPTS => {
                thread::sleep(retry_delay(attempt));
            }
            Err(error) => return Err(format!("FTShare news request failed: {error}")),
        }
    }

    Err("FTShare news request exhausted retries".into())
}

fn endpoint_url(base: &str) -> Result<reqwest::Url, String> {
    let normalized = format!("{}/", base.trim().trim_end_matches('/'));
    let base = reqwest::Url::parse(&normalized)
        .map_err(|error| format!("invalid FTSHARE_BASE_URL: {error}"))?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err("FTSHARE_BASE_URL must be an absolute http(s) URL".into());
    }
    base.join(ENDPOINT_PATH)
        .map_err(|error| format!("cannot construct FTShare news URL: {error}"))
}

fn query_params(config: &RendererConfig) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("query", config.query.clone()),
        ("limit", config.limit.to_string()),
    ];
    if let Some(start_time) = config.start_time.as_ref() {
        params.push(("start_time", start_time.clone()));
    }
    if let Some(end_time) = config.end_time.as_ref() {
        params.push(("end_time", end_time.clone()));
    }
    params
}

fn parse_response(
    mut response: Response,
    requested_limit: usize,
    endpoint: &str,
) -> Result<NewsDataset, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(format!(
            "FTShare news response exceeds the {MAX_RESPONSE_BYTES} byte limit"
        ));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read FTShare news response: {error}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "FTShare news response exceeds the {MAX_RESPONSE_BYTES} byte limit"
        ));
    }
    let raw: Vec<RawNewsItem> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid FTShare news response: {error}"))?;
    normalize_items(raw, requested_limit).map(|items| NewsDataset {
        items,
        source: endpoint.to_string(),
    })
}

fn normalize_items(raw: Vec<RawNewsItem>, requested_limit: usize) -> Result<Vec<NewsItem>, String> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for (index, raw) in raw.into_iter().enumerate() {
        if items.len() >= requested_limit {
            break;
        }
        let news_id = raw.news_id.into_string(index)?;
        if !seen.insert(news_id.clone()) {
            continue;
        }
        let article_url = clean_optional(raw.article_url, 2_048).and_then(valid_article_url);
        items.push(NewsItem {
            news_id,
            source_site: clean_optional(raw.source_site, 200)
                .unwrap_or_else(|| "Unknown source".into()),
            article_url,
            publish_time: clean_optional(raw.publish_time, 64),
            fetch_time: clean_optional(raw.fetch_time, 64),
            title: clean_optional(raw.title, 500).unwrap_or_else(|| "Untitled news".into()),
            media_name: clean_optional(raw.media_name, 200),
            summary: clean_optional(raw.summary, 4_000),
            content: clean_optional(raw.content, MAX_CONTENT_CHARS).unwrap_or_default(),
            is_truncated: raw.is_truncated.unwrap_or_default() != 0,
            is_reviewed: raw.is_reviewed.unwrap_or_default() != 0,
            score: raw.score.unwrap_or_default().clamp(0.0, 1.0),
        });
    }
    Ok(items)
}

impl WireNewsId {
    fn into_string(self, index: usize) -> Result<String, String> {
        let value = match self {
            Self::Unsigned(value) => value.to_string(),
            Self::String(value) => value.trim().to_string(),
        };
        if value.is_empty() || value.len() > 32 || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!(
                "FTShare news item {index} has an invalid decimal news_id"
            ));
        }
        Ok(value)
    }
}

fn clean_optional(value: Option<String>, max_chars: usize) -> Option<String> {
    let value = value?;
    let mut normalized = String::new();
    let mut whitespace = false;
    let mut character_count = 0;
    for character in value.chars() {
        if character.is_whitespace() || character.is_control() {
            whitespace = !normalized.is_empty();
            continue;
        }
        if whitespace {
            normalized.push(' ');
            whitespace = false;
        }
        if character_count >= max_chars {
            break;
        }
        normalized.push(character);
        character_count += 1;
    }
    let normalized = normalized.trim().to_string();
    (!normalized.is_empty()).then_some(normalized)
}

fn valid_article_url(value: String) -> Option<String> {
    let parsed = reqwest::Url::parse(&value).ok()?;
    if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some() {
        Some(parsed.to_string())
    } else {
        None
    }
}

fn retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 502 | 503 | 504)
}

fn retryable_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(match attempt {
        0 => 200,
        _ => 600,
    })
}

fn response_error(mut response: Response) -> String {
    let status = response.status();
    let log_id = response
        .headers()
        .get("x-ft-logid")
        .and_then(|value| value.to_str().ok())
        .map(|value| format!("; x-ft-logid={value}"))
        .unwrap_or_default();
    let mut bytes = Vec::new();
    let _ = response.by_ref().take(1_024).read_to_end(&mut bytes);
    let body = String::from_utf8_lossy(&bytes);
    let body = body.trim();
    if body.is_empty() {
        format!("FTShare news request returned HTTP {status}{log_id}")
    } else {
        format!("FTShare news request returned HTTP {status}{log_id}: {body}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse_raw(value: serde_json::Value) -> Vec<RawNewsItem> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn normalizes_large_numeric_ids_and_nullable_fields() {
        let raw = parse_raw(json!([{
            "news_id": 606732245083885569_u64,
            "source_site": "人民网_快讯",
            "article_url": "http://finance.people.com.cn/a",
            "publish_time": "2026-08-02T07:48:00",
            "title": null,
            "summary": null,
            "content": "  first\n\nsecond  ",
            "is_truncated": 0,
            "is_reviewed": 1,
            "score": 0.7395
        }]));
        let items = normalize_items(raw, 20).unwrap();
        assert_eq!(items[0].news_id, "606732245083885569");
        assert_eq!(items[0].title, "Untitled news");
        assert_eq!(items[0].content, "first second");
        assert!(items[0].article_url.is_some());
    }

    #[test]
    fn rejects_invalid_ids_and_unsafe_urls() {
        let raw = parse_raw(json!([{
            "news_id": "not-a-number",
            "article_url": "javascript:alert(1)"
        }]));
        assert!(normalize_items(raw, 20).is_err());

        let raw = parse_raw(json!([{
            "news_id": "123",
            "article_url": "javascript:alert(1)"
        }]));
        assert!(normalize_items(raw, 20).unwrap()[0].article_url.is_none());
    }

    #[test]
    fn constructs_gateway_url_and_query_params() {
        let endpoint = endpoint_url("https://market.ft.tech/gateway").unwrap();
        assert_eq!(
            endpoint.as_str(),
            "https://market.ft.tech/gateway/api/v1/market/data/semantic-search-news"
        );
        let config = RendererConfig::from_value(json!({
            "query": "人工智能",
            "limit": 5,
            "startTime": "2026-08-01T00:00:00+08:00"
        }))
        .unwrap();
        assert_eq!(
            query_params(&config),
            vec![
                ("query", "人工智能".into()),
                ("limit", "5".into()),
                ("start_time", "2026-08-01T00:00:00+08:00".into())
            ]
        );
    }

    #[test]
    fn retries_only_transient_http_statuses() {
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!retryable_status(StatusCode::BAD_REQUEST));
        assert!(!retryable_status(StatusCode::NOT_FOUND));
    }
}
