use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_LIMIT: usize = 120;
const MAX_LIMIT: usize = 2_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRendererConfig {
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data_file: Option<PathBuf>,
    #[serde(default)]
    interval_unit: Option<IntervalUnit>,
    #[serde(default)]
    interval_value: Option<u32>,
    #[serde(default)]
    adjust_kind: Option<AdjustKind>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum IntervalUnit {
    Minute,
    Day,
    Week,
    Month,
    Year,
}

impl IntervalUnit {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Minute => "Minute",
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum AdjustKind {
    None,
    Forward,
    Backward,
}

impl AdjustKind {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Forward => "Forward",
            Self::Backward => "Backward",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RendererConfig {
    pub tag: String,
    pub data_file: Option<PathBuf>,
    pub interval_unit: IntervalUnit,
    pub interval_value: u32,
    pub adjust_kind: AdjustKind,
    pub limit: usize,
}

impl RendererConfig {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let raw: RawRendererConfig = serde_json::from_value(value).map_err(|error| {
            format!(
                "invalid renderer config: {error}. Expected either {{\"tag\":\"000001.SZ\"}} or its alias {{\"code\":\"000001.SZ\"}}; inline OHLCV data is not accepted"
            )
        })?;

        let identifier = match (raw.tag, raw.code) {
            (Some(tag), None) | (None, Some(tag)) => tag,
            (Some(_), Some(_)) => {
                return Err(
                    "config must use exactly one stock identifier field: either \"tag\" or \"code\", not both"
                        .to_string(),
                );
            }
            (None, None) => {
                return Err(
                    "config requires a stock identifier in either \"tag\" or its alias \"code\""
                        .to_string(),
                );
            }
        };
        let tag = normalize_stock_tag(&identifier)?;
        let interval_value = raw.interval_value.unwrap_or(1);
        if interval_value == 0 || interval_value > 10_000 {
            return Err("interval_value must be between 1 and 10000".to_string());
        }

        let limit = raw.limit.unwrap_or(DEFAULT_LIMIT);
        if limit == 0 || limit > MAX_LIMIT {
            return Err(format!("limit must be between 1 and {MAX_LIMIT}"));
        }

        if let Some(path) = raw.data_file.as_ref()
            && path.as_os_str().is_empty()
        {
            return Err("data_file must not be empty".to_string());
        }

        Ok(Self {
            tag,
            data_file: raw.data_file,
            interval_unit: raw.interval_unit.unwrap_or(IntervalUnit::Day),
            interval_value,
            adjust_kind: raw.adjust_kind.unwrap_or(AdjustKind::None),
            limit,
        })
    }

    pub fn timeframe(&self) -> String {
        format!(
            "{} {}",
            self.interval_value,
            self.interval_unit.as_api_str()
        )
    }
}

pub fn normalize_stock_tag(input: &str) -> Result<String, String> {
    let tag = input.trim().to_ascii_uppercase();
    let Some((code, exchange)) = tag.split_once('.') else {
        return Err(
            "stock identifier in tag/code must use the six-digit exchange-qualified form, for example 000001.SZ"
                .to_string(),
        );
    };

    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("stock identifier in tag/code must start with exactly six digits".to_string());
    }
    if !matches!(exchange, "SZ" | "SH" | "BJ") {
        return Err("stock identifier exchange must be SZ, SH, or BJ".to_string());
    }
    if tag.matches('.').count() != 1 {
        return Err("stock identifier must contain exactly one exchange separator".to_string());
    }

    let prefix = &code[..3];
    let exchange_matches_code = match exchange {
        "SZ" => matches!(
            prefix,
            "000" | "001" | "002" | "003" | "200" | "300" | "301"
        ),
        "SH" => matches!(
            prefix,
            "600" | "601" | "603" | "605" | "688" | "689" | "900"
        ),
        "BJ" => matches!(code.as_bytes()[0], b'4' | b'8' | b'9'),
        _ => false,
    };
    if !exchange_matches_code {
        return Err(format!(
            "stock identifier {tag} does not match a recognized mainland stock code prefix for {exchange}"
        ));
    }

    Ok(tag)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_tag_only_with_api_defaults() {
        let config = RendererConfig::from_value(json!({ "tag": "000001.sz" })).unwrap();
        assert_eq!(config.tag, "000001.SZ");
        assert_eq!(config.interval_unit, IntervalUnit::Day);
        assert_eq!(config.interval_value, 1);
        assert_eq!(config.limit, 120);
        assert!(config.data_file.is_none());
    }

    #[test]
    fn accepts_file_reference_without_inline_data() {
        let config = RendererConfig::from_value(json!({
            "tag": "600519.SH",
            "data_file": "/tmp/600519.json",
            "interval_unit": "Week"
        }))
        .unwrap();
        assert_eq!(config.data_file, Some(PathBuf::from("/tmp/600519.json")));
        assert_eq!(config.interval_unit, IntervalUnit::Week);
    }

    #[test]
    fn rejects_inline_candles() {
        let error = RendererConfig::from_value(json!({
            "tag": "000001.SZ",
            "candles": []
        }))
        .unwrap_err();
        assert!(error.contains("inline OHLCV data is not accepted"));
    }

    #[test]
    fn rejects_malformed_tag() {
        assert!(normalize_stock_tag("AAPL").is_err());
        assert!(normalize_stock_tag("000001.NY").is_err());
        assert!(normalize_stock_tag("00001.SZ").is_err());
        assert!(normalize_stock_tag("000001.SH").is_err());
    }
}
