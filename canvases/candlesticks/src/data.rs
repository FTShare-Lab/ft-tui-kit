use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use crate::config::RendererConfig;

const DEFAULT_MARKET_URL: &str =
    "https://market.ft.tech/gateway/api/v1/market/data/stock-candlesticks";
const MAX_DATA_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MarketBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub ts_millis: i64,
    pub ts_millis_open: Option<i64>,
    pub turnover: Option<f64>,
    pub volume: f64,
}

#[derive(Debug, Clone)]
pub struct MarketDataset {
    pub config: RendererConfig,
    pub bars: Vec<MarketBar>,
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct RawMarketBar {
    #[serde(deserialize_with = "deserialize_f64")]
    open: f64,
    #[serde(deserialize_with = "deserialize_f64")]
    high: f64,
    #[serde(deserialize_with = "deserialize_f64")]
    low: f64,
    #[serde(deserialize_with = "deserialize_f64")]
    close: f64,
    #[serde(
        alias = "timestamp",
        alias = "time",
        deserialize_with = "deserialize_i64"
    )]
    ts_millis: i64,
    #[serde(
        default,
        alias = "timestamp_open",
        deserialize_with = "deserialize_option_i64"
    )]
    ts_millis_open: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_option_f64")]
    turnover: Option<f64>,
    #[serde(deserialize_with = "deserialize_f64")]
    volume: f64,
}

impl From<RawMarketBar> for MarketBar {
    fn from(value: RawMarketBar) -> Self {
        Self {
            open: value.open,
            high: value.high,
            low: value.low,
            close: value.close,
            ts_millis: value.ts_millis,
            ts_millis_open: value.ts_millis_open,
            turnover: value.turnover,
            volume: value.volume,
        }
    }
}

pub fn load_market_data(config: RendererConfig) -> Result<MarketDataset, String> {
    let (bars, source) = match config.data_file.as_ref() {
        Some(path) => (load_file(path)?, format!("file:{}", path.display())),
        None => (fetch_api(&config)?, "market.ft.tech".to_string()),
    };

    let bars = normalize_and_validate(bars)?;
    if bars.is_empty() {
        return Err(format!("no candlesticks were found for {}", config.tag));
    }

    Ok(MarketDataset {
        config,
        bars,
        source,
    })
}

fn load_file(path: &Path) -> Result<Vec<MarketBar>, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("cannot inspect data_file {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "data_file is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_DATA_FILE_BYTES {
        return Err(format!(
            "data_file exceeds the {} MiB limit: {}",
            MAX_DATA_FILE_BYTES / (1024 * 1024),
            path.display()
        ));
    }

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => load_json_file(path),
        Some("csv") => load_csv_file(path),
        _ => Err("data_file must have a .json or .csv extension".to_string()),
    }
}

fn load_json_file(path: &Path) -> Result<Vec<MarketBar>, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open JSON data_file {}: {error}", path.display()))?;
    let value: Value = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("invalid JSON data_file {}: {error}", path.display()))?;
    parse_bars_value(&value)
}

fn load_csv_file(path: &Path) -> Result<Vec<MarketBar>, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open CSV data_file {}: {error}", path.display()))?;
    let mut reader = csv::Reader::from_reader(BufReader::new(file));
    reader
        .deserialize::<RawMarketBar>()
        .enumerate()
        .map(|(index, row)| {
            row.map(MarketBar::from).map_err(|error| {
                format!(
                    "invalid CSV row {} in {}: {error}",
                    index + 2,
                    path.display()
                )
            })
        })
        .collect()
}

fn fetch_api(config: &RendererConfig) -> Result<Vec<MarketBar>, String> {
    let endpoint = std::env::var("FINANCIAL_CANVAS_MARKET_URL")
        .unwrap_or_else(|_| DEFAULT_MARKET_URL.to_string());
    let until_ts_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis() as i64;

    let body = json!({
        "symbol": config.tag,
        "interval_unit": config.interval_unit.as_api_str(),
        "interval_value": config.interval_value,
        "adjust_kind": config.adjust_kind.as_api_str(),
        "until_ts_millis": until_ts_millis,
        "limit": config.limit
    });

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("cannot create market API client: {error}"))?;
    let response = client
        .post(&endpoint)
        .json(&body)
        .send()
        .map_err(|error| format!("market API request failed for {}: {error}", config.tag))?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(MAX_DATA_FILE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read market API response: {error}"))?;
    if !status.is_success() {
        let message = String::from_utf8_lossy(&bytes);
        return Err(format!(
            "market API rejected {} with HTTP {}: {}",
            config.tag,
            status,
            truncate(&message, 500)
        ));
    }

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("market API returned invalid JSON: {error}"))?;
    parse_bars_value(&value).map_err(|error| {
        format!(
            "market API returned unusable data for {}: {error}",
            config.tag
        )
    })
}

fn parse_bars_value(value: &Value) -> Result<Vec<MarketBar>, String> {
    let rows = locate_rows(value).ok_or_else(|| {
        "expected an array, or an object containing data/result/items/rows/candles".to_string()
    })?;
    rows.iter()
        .enumerate()
        .map(|(index, value)| {
            serde_json::from_value::<RawMarketBar>(value.clone())
                .map(MarketBar::from)
                .map_err(|error| format!("invalid candlestick at index {index}: {error}"))
        })
        .collect()
}

fn locate_rows(value: &Value) -> Option<&Vec<Value>> {
    if let Value::Array(rows) = value {
        return Some(rows);
    }

    let object = value.as_object()?;
    for key in ["data", "result", "items", "rows", "candles"] {
        let Some(candidate) = object.get(key) else {
            continue;
        };
        if let Some(rows) = candidate.as_array() {
            return Some(rows);
        }
        if let Some(rows) = locate_rows(candidate) {
            return Some(rows);
        }
    }
    None
}

fn normalize_and_validate(mut bars: Vec<MarketBar>) -> Result<Vec<MarketBar>, String> {
    bars.sort_by_key(|bar| bar.ts_millis);
    bars.dedup_by_key(|bar| bar.ts_millis);

    for (index, bar) in bars.iter().enumerate() {
        let prices = [bar.open, bar.high, bar.low, bar.close, bar.volume];
        if prices.iter().any(|value| !value.is_finite()) {
            return Err(format!("candlestick {index} contains a non-finite number"));
        }
        if bar.ts_millis <= 0 {
            return Err(format!("candlestick {index} has an invalid ts_millis"));
        }
        if bar.low > bar.open.min(bar.close) || bar.high < bar.open.max(bar.close) {
            return Err(format!(
                "candlestick {index} violates OHLC bounds: low <= open/close <= high is required"
            ));
        }
        if bar.high < bar.low {
            return Err(format!("candlestick {index} has high below low"));
        }
        if bar.volume < 0.0 {
            return Err(format!("candlestick {index} has negative volume"));
        }
        if let Some(turnover) = bar.turnover
            && (!turnover.is_finite() || turnover < 0.0)
        {
            return Err(format!("candlestick {index} has invalid turnover"));
        }
    }

    Ok(bars)
}

fn deserialize_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    flexible_f64(&value)
        .ok_or_else(|| serde::de::Error::custom("expected a number or numeric string"))
}

fn deserialize_option_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value
        .map(|value| {
            flexible_f64(&value)
                .ok_or_else(|| serde::de::Error::custom("expected a number or numeric string"))
        })
        .transpose()
}

fn deserialize_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    flexible_i64(&value)
        .ok_or_else(|| serde::de::Error::custom("expected an integer or integer string"))
}

fn deserialize_option_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value
        .map(|value| {
            flexible_i64(&value)
                .ok_or_else(|| serde::de::Error::custom("expected an integer or integer string"))
        })
        .transpose()
}

fn flexible_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn flexible_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::json;
    use tempfile::Builder;

    use super::*;

    #[test]
    fn parses_api_decimal_strings_and_wrapper() {
        let value = json!({
            "data": [{
                "open": "10.52",
                "high": "10.67",
                "low": "10.42",
                "close": "10.65",
                "ts_millis": 1782111600000_i64,
                "ts_millis_open": 1782091800000_i64,
                "turnover": "2669536105.56",
                "volume": 253063344
            }]
        });
        let bars = parse_bars_value(&value).unwrap();
        assert_eq!(bars[0].close, 10.65);
        assert_eq!(bars[0].volume, 253063344.0);
    }

    #[test]
    fn loads_csv_file() {
        let mut file = Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, "open,high,low,close,ts_millis,volume").unwrap();
        writeln!(file, "10,12,9,11,1782111600000,1000").unwrap();
        let bars = load_file(file.path()).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].high, 12.0);
    }

    #[test]
    fn rejects_invalid_ohlc_bounds() {
        let result = normalize_and_validate(vec![MarketBar {
            open: 10.0,
            high: 9.0,
            low: 8.0,
            close: 10.0,
            ts_millis: 1,
            ts_millis_open: None,
            turnover: None,
            volume: 1.0,
        }]);
        assert!(result.is_err());
    }
}
