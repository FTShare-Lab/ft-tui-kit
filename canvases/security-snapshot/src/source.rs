use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_SECURITY_URL: &str = "https://ftai.chat";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererConfig {
    symbol: String,
}

pub fn load(value: Value) -> Result<Value, String> {
    let config: RendererConfig = serde_json::from_value(value).map_err(|error| {
        format!(
            "invalid security-snapshot config: {error}. Only {{\"symbol\":\"600519.SH\"}} is accepted"
        )
    })?;
    let symbol = normalize_stock_symbol(&config.symbol)?;
    let base =
        std::env::var("FTSHARE_SECURITY_BASE_URL").unwrap_or_else(|_| DEFAULT_SECURITY_URL.into());
    let url = format!(
        "{}/api/v1/market/security/{symbol}/info",
        base.trim_end_matches('/')
    );
    let response = reqwest::blocking::get(url)
        .map_err(|error| format!("FTShare security request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("FTShare security request returned HTTP {status}"));
    }
    let mut value: Value = response
        .json()
        .map_err(|error| format!("invalid FTShare security response: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "FTShare security response must be an object".to_string())?;
    object.insert("schema_version".into(), json!(1));
    object.insert("source".into(), json!("FTShare/stock-security-info"));
    Ok(value)
}

fn normalize_stock_symbol(input: &str) -> Result<String, String> {
    let symbol = input.trim().to_ascii_uppercase();
    let Some((code, exchange)) = symbol.split_once('.') else {
        return Err("symbol must be an exchange-qualified A-share code such as 600519.SH".into());
    };
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("symbol must start with exactly six digits".into());
    }
    if !matches!(exchange, "SH" | "SZ" | "BJ") || symbol.matches('.').count() != 1 {
        return Err("symbol exchange must be SH, SZ, or BJ".into());
    }
    Ok(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_a_share_symbol() {
        assert_eq!(normalize_stock_symbol("600519.sh").unwrap(), "600519.SH");
    }

    #[test]
    fn rejects_non_a_share_symbol() {
        assert!(normalize_stock_symbol("AAPL").is_err());
    }
}
