use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRendererConfig {
    data_file: Option<PathBuf>,
    #[serde(rename = "dataFile")]
    data_file_camel: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub data_file: PathBuf,
}

impl RendererConfig {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let raw: RawRendererConfig = serde_json::from_value(value).map_err(|error| {
            format!(
                "invalid renderer config: {error}. Expected {{\"data_file\":\"/absolute/path/chart.json\"}}"
            )
        })?;

        let data_file = match (raw.data_file, raw.data_file_camel) {
            (Some(path), None) | (None, Some(path)) => path,
            (Some(_), Some(_)) => {
                return Err(
                    "config must use exactly one data path field: data_file or dataFile, not both"
                        .to_string(),
                );
            }
            (None, None) => {
                return Err(
                    "config requires data_file (or dataFile) with an absolute JSON file path"
                        .to_string(),
                );
            }
        };

        if data_file.as_os_str().is_empty() {
            return Err("data_file must not be empty".to_string());
        }
        if !data_file.is_absolute() {
            return Err(format!(
                "data_file must be an absolute path, got {}",
                data_file.display()
            ));
        }
        if data_file
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some("json")
        {
            return Err("data_file must have a .json extension".to_string());
        }

        Ok(Self { data_file })
    }
}
