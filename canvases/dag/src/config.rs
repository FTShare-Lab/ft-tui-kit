use std::path::PathBuf;

use serde_json::Value;

#[derive(Debug, Clone)]
pub enum RendererSource {
    Inline(Value),
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub source: RendererSource,
}

impl RendererConfig {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "DAG config must be a JSON object".to_string())?;
        let snake = object.get("data_file");
        let camel = object.get("dataFile");

        match (snake, camel) {
            (Some(_), Some(_)) => Err(
                "config must use exactly one file path field: data_file or dataFile, not both"
                    .to_string(),
            ),
            (Some(path), None) | (None, Some(path)) => {
                if object.len() != 1 {
                    return Err(
                        "file-based DAG config may contain only data_file or dataFile; do not mix file and inline fields"
                            .to_string(),
                    );
                }
                let path = path
                    .as_str()
                    .ok_or_else(|| "data_file must be a string".to_string())?;
                let path = PathBuf::from(path);
                if path.as_os_str().is_empty() {
                    return Err("data_file must not be empty".to_string());
                }
                if !path.is_absolute() {
                    return Err(format!(
                        "data_file must be an absolute path, got {}",
                        path.display()
                    ));
                }
                if path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                    != Some("json")
                {
                    return Err("data_file must have a .json extension".to_string());
                }
                Ok(Self {
                    source: RendererSource::File(path),
                })
            }
            (None, None) => {
                if object.is_empty() {
                    return Err(
                        "config must be an inline DAG document or contain data_file/dataFile"
                            .to_string(),
                    );
                }
                Ok(Self {
                    source: RendererSource::Inline(value),
                })
            }
        }
    }
}
