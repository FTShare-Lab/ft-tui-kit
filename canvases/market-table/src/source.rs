use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_MARKET_URL: &str = "https://market.ft.tech/gateway";
const MAX_DATA_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererConfig {
    #[serde(default)]
    preset: Option<Preset>,
    #[serde(default)]
    data_file: Option<PathBuf>,
    #[serde(default, rename = "dataFile")]
    data_file_camel: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Preset {
    AShareTopGainers,
    AShareTopLosers,
    AShareMostActive,
    AShareHighestTurnoverRate,
    AShareLargestMarketCap,
}

impl Preset {
    fn title(self) -> &'static str {
        match self {
            Self::AShareTopGainers => "A-share Top Gainers",
            Self::AShareTopLosers => "A-share Top Losers",
            Self::AShareMostActive => "A-share Most Active",
            Self::AShareHighestTurnoverRate => "A-share Highest Turnover Rate",
            Self::AShareLargestMarketCap => "A-share Largest Market Cap",
        }
    }

    fn order_by(self) -> &'static str {
        match self {
            Self::AShareTopGainers => "change_rate%20desc",
            Self::AShareTopLosers => "change_rate%20asc",
            Self::AShareMostActive => "turnover%20desc",
            Self::AShareHighestTurnoverRate => "turnover_rate%20desc",
            Self::AShareLargestMarketCap => "market_cap%20desc",
        }
    }
}

pub fn load(value: Value) -> Result<Value, String> {
    let config: RendererConfig = serde_json::from_value(value).map_err(|error| {
        format!(
            "invalid market-table config: {error}. Use exactly one of preset or data_file; inline rows are not accepted"
        )
    })?;
    let data_file = match (config.data_file, config.data_file_camel) {
        (Some(path), None) | (None, Some(path)) => Some(path),
        (Some(_), Some(_)) => return Err("use data_file or dataFile, not both".into()),
        (None, None) => None,
    };
    match (config.preset, data_file) {
        (Some(preset), None) => fetch_preset(preset),
        (None, Some(path)) => load_file(&path),
        (Some(_), Some(_)) => Err("use exactly one of preset or data_file".into()),
        (None, None) => Err("config requires preset or data_file".into()),
    }
}

fn load_file(path: &Path) -> Result<Value, String> {
    if !path.is_absolute() {
        return Err("data_file must be an absolute path".into());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect data_file {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "data_file is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_DATA_FILE_BYTES {
        return Err("data_file exceeds the 16 MiB limit".into());
    }
    serde_json::from_reader(
        std::fs::File::open(path)
            .map_err(|error| format!("cannot open data_file {}: {error}", path.display()))?,
    )
    .map_err(|error| format!("data_file contains invalid JSON: {error}"))
}

fn fetch_preset(preset: Preset) -> Result<Value, String> {
    let base = std::env::var("FTSHARE_BASE_URL").unwrap_or_else(|_| DEFAULT_MARKET_URL.into());
    let url = format!(
        "{}/api/v1/market/data/daec/stocks/all?page=1&page_size=20&order_by={}",
        base.trim_end_matches('/'),
        preset.order_by()
    );
    let response = reqwest::blocking::Client::new()
        .get(url)
        .header("X-Client-Name", "ft-claw")
        .send()
        .map_err(|error| format!("FTShare preset request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("FTShare preset request returned HTTP {status}"));
    }
    let mut value: Value = response
        .json()
        .map_err(|error| format!("invalid FTShare preset response: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "FTShare preset response must be an object".to_string())?;
    object.insert("schemaVersion".into(), json!(1));
    object.insert("title".into(), json!(preset.title()));
    object.insert("subtitle".into(), json!("Top 20 · A-share market"));
    object.insert("source".into(), json!("FTShare/stock-daec-stocks"));
    Ok(value)
}
