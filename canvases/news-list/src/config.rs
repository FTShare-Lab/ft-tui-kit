use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_LIMIT: usize = 20;
const MAX_QUERY_CHARS: usize = 200;
const MAX_HIGHLIGHTS: usize = 50;
const MAX_REASON_CHARS: usize = 240;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RendererConfig {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<NewsHighlight>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewsHighlight {
    pub news_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

impl RendererConfig {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let mut config: Self = serde_json::from_value(value)
            .map_err(|error| format!("invalid news-list config: {error}"))?;
        config.query = config.query.trim().to_string();
        if config.query.is_empty() {
            return Err("query cannot be empty".into());
        }
        if config.query.chars().count() > MAX_QUERY_CHARS {
            return Err(format!("query cannot exceed {MAX_QUERY_CHARS} characters"));
        }
        if !(1..=50).contains(&config.limit) {
            return Err("limit must be between 1 and 50".into());
        }
        config.start_time = normalize_optional("startTime", config.start_time)?;
        config.end_time = normalize_optional("endTime", config.end_time)?;
        if config.highlights.len() > MAX_HIGHLIGHTS {
            return Err(format!("highlights cannot exceed {MAX_HIGHLIGHTS} items"));
        }

        let mut ids = BTreeSet::new();
        for (index, highlight) in config.highlights.iter_mut().enumerate() {
            highlight.news_id = highlight.news_id.trim().to_string();
            if highlight.news_id.is_empty()
                || highlight.news_id.len() > 32
                || !highlight.news_id.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(format!(
                    "highlights[{index}].newsId must be a decimal string of at most 32 digits"
                ));
            }
            if !ids.insert(highlight.news_id.clone()) {
                return Err(format!(
                    "highlights contains duplicate newsId {}",
                    highlight.news_id
                ));
            }
            highlight.reason = normalize_reason(index, highlight.reason.take())?;
        }
        Ok(config)
    }

    pub fn same_search(&self, other: &Self) -> bool {
        self.query == other.query
            && self.limit == other.limit
            && self.start_time == other.start_time
            && self.end_time == other.end_time
    }

    pub fn with_query(&self, query: String) -> Result<Self, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| "news-list config must be an object".to_string())?;
        object.insert("query".into(), Value::String(query));
        object.insert("highlights".into(), Value::Array(Vec::new()));
        Self::from_value(value)
    }
}

fn normalize_optional(name: &str, value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{name} cannot be empty when provided"));
    }
    if value.len() > 64 || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} must be a valid ISO 8601 value of at most 64 characters"
        ));
    }
    Ok(Some(value))
}

fn normalize_reason(index: usize, value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.chars().count() > MAX_REASON_CHARS {
        return Err(format!(
            "highlights[{index}].reason cannot exceed {MAX_REASON_CHARS} characters"
        ));
    }
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_defaults_and_trims_query() {
        let config = RendererConfig::from_value(json!({ "query": "  人工智能  " })).unwrap();
        assert_eq!(config.query, "人工智能");
        assert_eq!(config.limit, 20);
        assert!(config.highlights.is_empty());
    }

    #[test]
    fn preserves_large_news_ids_as_strings() {
        let config = RendererConfig::from_value(json!({
            "query": "AI",
            "highlights": [{
                "newsId": "606732245083885569",
                "reason": "  Material policy signal  "
            }]
        }))
        .unwrap();
        assert_eq!(config.highlights[0].news_id, "606732245083885569");
        assert_eq!(
            config.highlights[0].reason.as_deref(),
            Some("Material policy signal")
        );
    }

    #[test]
    fn rejects_numeric_or_duplicate_highlight_ids() {
        assert!(
            RendererConfig::from_value(json!({
                "query": "AI",
                "highlights": [{"newsId": 606732245083885569_u64}]
            }))
            .is_err()
        );
        assert!(
            RendererConfig::from_value(json!({
                "query": "AI",
                "highlights": [{"newsId": "1"}, {"newsId": "1"}]
            }))
            .is_err()
        );
    }

    #[test]
    fn search_identity_ignores_highlights() {
        let first = RendererConfig::from_value(json!({"query":"AI"})).unwrap();
        let second = RendererConfig::from_value(json!({
            "query":"AI",
            "highlights":[{"newsId":"1"}]
        }))
        .unwrap();
        assert!(first.same_search(&second));
    }
}
