use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::RendererConfig;

const MAX_DATA_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_SERIES: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartDocument {
    pub schema_version: u8,
    pub title: String,
    pub subtitle: Option<String>,
    pub table: TableSpec,
    pub axes: AxesSpec,
    pub series: Vec<SeriesSpec>,
    pub display: DisplaySpec,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableSpec {
    pub name: String,
    pub id_field: Option<String>,
    pub rows: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxesSpec {
    pub x: XAxisSpec,
    pub y: YAxisSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XAxisSpec {
    pub field: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YAxisSpec {
    pub label: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub format: NumberFormat,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberFormat {
    #[serde(default)]
    pub decimals: u32,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesSpec {
    pub field: String,
    pub label: String,
    #[serde(default = "default_color")]
    pub color: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShowValues {
    Never,
    Selected,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartView {
    Bar,
    Line,
    Both,
}

impl ChartView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Line => "line",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplaySpec {
    pub view: ChartView,
    #[serde(default = "default_true")]
    pub show_legend: bool,
    #[serde(default = "default_show_values")]
    pub show_values: ShowValues,
    #[serde(default = "default_bar_width")]
    pub bar_width: u16,
    #[serde(default = "default_bar_gap")]
    pub bar_gap: u16,
    #[serde(default = "default_group_gap")]
    pub group_gap: u16,
}

#[derive(Debug, Clone)]
pub struct ChartDataset {
    pub config: RendererConfig,
    pub document: ChartDocument,
    pub categories: Vec<String>,
    pub values: Vec<Vec<f64>>,
    pub source: String,
}

impl ChartDataset {
    pub fn row_count(&self) -> usize {
        self.values.len()
    }

    pub fn series_count(&self) -> usize {
        self.document.series.len()
    }

    pub fn value(&self, row: usize, series: usize) -> Option<f64> {
        self.values
            .get(row)
            .and_then(|values| values.get(series))
            .copied()
    }

    pub fn scale(&self) -> u64 {
        10_u64.pow(self.document.axes.y.format.decimals)
    }

    pub fn scaled_value(&self, row: usize, series: usize) -> u64 {
        let value = self.value(row, series).unwrap_or_default();
        (value * self.scale() as f64).round() as u64
    }

    pub fn axis_max(&self, rows: Range<usize>) -> f64 {
        if let Some(maximum) = self.document.axes.y.max {
            return maximum;
        }
        let observed = self.values[rows]
            .iter()
            .flatten()
            .copied()
            .fold(0.0_f64, f64::max);
        if observed == 0.0 {
            1.0
        } else {
            observed * 1.05
        }
    }

    pub fn format_value(&self, value: f64) -> String {
        let format = &self.document.axes.y.format;
        format!(
            "{}{:.*}{}",
            format.prefix, format.decimals as usize, value, format.suffix
        )
    }
}

pub fn load_chart_data(config: RendererConfig) -> Result<ChartDataset, String> {
    let metadata = config.data_file.metadata().map_err(|error| {
        format!(
            "cannot inspect data_file {}: {error}",
            config.data_file.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "data_file is not a regular file: {}",
            config.data_file.display()
        ));
    }
    if metadata.len() > MAX_DATA_FILE_BYTES {
        return Err(format!(
            "data_file exceeds the {} MiB limit: {}",
            MAX_DATA_FILE_BYTES / (1024 * 1024),
            config.data_file.display()
        ));
    }

    let file = File::open(&config.data_file).map_err(|error| {
        format!(
            "cannot open data_file {}: {error}",
            config.data_file.display()
        )
    })?;
    let raw: Value = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("data_file contains invalid JSON: {error}"))?;
    validate_required_fields(&raw)?;
    let document: ChartDocument = serde_json::from_value(raw)
        .map_err(|error| format!("chart document contains an invalid field: {error}"))?;
    let (categories, values) = validate_document(&document)?;
    let source = document
        .metadata
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("file:{}", config.data_file.display()));

    Ok(ChartDataset {
        config,
        document,
        categories,
        values,
        source,
    })
}

fn validate_required_fields(value: &Value) -> Result<(), String> {
    let document = require_object(value, "document")?;
    require_field(document, "schemaVersion", "")?;
    require_field(document, "title", "")?;

    let table = require_object(require_field(document, "table", "")?, "table")?;
    require_field(table, "name", "table")?;
    require_array(require_field(table, "rows", "table")?, "table.rows")?;

    let axes = require_object(require_field(document, "axes", "")?, "axes")?;
    let x_axis = require_object(require_field(axes, "x", "axes")?, "axes.x")?;
    require_field(x_axis, "field", "axes.x")?;
    require_object(require_field(axes, "y", "axes")?, "axes.y")?;

    let series = require_array(require_field(document, "series", "")?, "series")?;
    for (index, value) in series.iter().enumerate() {
        let path = format!("series[{index}]");
        let entry = require_object(value, &path)?;
        require_field(entry, "field", &path)?;
        require_field(entry, "label", &path)?;
    }

    let display = require_object(require_field(document, "display", "")?, "display")?;
    let view = require_field(display, "view", "display")?;
    match view.as_str() {
        Some("bar" | "line" | "both") => {}
        Some(value) => {
            return Err(format!(
                "field `display.view` must be `bar`, `line`, or `both`; got `{value}`"
            ));
        }
        None => return Err("field `display.view` must be a string".to_string()),
    }

    Ok(())
}

fn require_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    parent: &str,
) -> Result<&'a Value, String> {
    object.get(field).ok_or_else(|| {
        let path = if parent.is_empty() {
            field.to_string()
        } else {
            format!("{parent}.{field}")
        };
        format!("required field `{path}` is missing")
    })
}

fn require_object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("field `{path}` must be an object"))
}

fn require_array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("field `{path}` must be an array"))
}

fn validate_document(document: &ChartDocument) -> Result<(Vec<String>, Vec<Vec<f64>>), String> {
    if document.schema_version != 1 {
        return Err(format!(
            "unsupported schemaVersion {}, expected 1",
            document.schema_version
        ));
    }
    if document.title.trim().is_empty() || document.table.name.trim().is_empty() {
        return Err("title and table.name must not be empty".to_string());
    }
    if document.title.chars().count() > 160 {
        return Err("title must not exceed 160 characters".to_string());
    }
    if document
        .subtitle
        .as_ref()
        .is_some_and(|subtitle| subtitle.chars().count() > 240)
    {
        return Err("subtitle must not exceed 240 characters".to_string());
    }
    if document.table.name.chars().count() > 120 {
        return Err("table.name must not exceed 120 characters".to_string());
    }
    if document
        .table
        .id_field
        .as_ref()
        .is_some_and(|field| field.trim().is_empty())
    {
        return Err("table.idField must not be empty".to_string());
    }
    if document.table.rows.is_empty() || document.table.rows.len() > MAX_ROWS {
        return Err(format!("table.rows must contain 1 to {MAX_ROWS} rows"));
    }
    if document.series.is_empty() || document.series.len() > MAX_SERIES {
        return Err(format!("series must contain 1 to {MAX_SERIES} entries"));
    }
    if document.axes.x.field.trim().is_empty() {
        return Err("axes.x.field must not be empty".to_string());
    }
    if document
        .axes
        .x
        .label
        .as_ref()
        .is_some_and(|label| label.chars().count() > 80)
        || document
            .axes
            .y
            .label
            .as_ref()
            .is_some_and(|label| label.chars().count() > 80)
    {
        return Err("axis labels must not exceed 80 characters".to_string());
    }
    if let Some(minimum) = document.axes.y.min
        && minimum != 0.0
    {
        return Err(
            "axes.y.min must be 0 because Ratatui BarChart uses a zero baseline".to_string(),
        );
    }
    if let Some(maximum) = document.axes.y.max
        && (!maximum.is_finite() || maximum <= 0.0)
    {
        return Err("axes.y.max must be a finite number greater than zero".to_string());
    }
    if document.axes.y.format.decimals > 6 {
        return Err("axes.y.format.decimals must be between 0 and 6".to_string());
    }
    if document.axes.y.format.prefix.chars().count() > 12
        || document.axes.y.format.suffix.chars().count() > 12
    {
        return Err("axes.y.format prefix and suffix must not exceed 12 characters".to_string());
    }
    if !(1..=8).contains(&document.display.bar_width) {
        return Err("display.barWidth must be between 1 and 8".to_string());
    }
    if document.display.bar_gap > 8 || document.display.group_gap > 12 {
        return Err("display.barGap must be at most 8 and groupGap at most 12".to_string());
    }

    for (key, value) in &document.metadata {
        if !is_scalar(value) {
            return Err(format!("metadata.{key} must be a scalar value"));
        }
    }

    let mut fields = HashSet::new();
    for (index, series) in document.series.iter().enumerate() {
        if series.field.trim().is_empty() || series.label.trim().is_empty() {
            return Err(format!("series[{index}] field and label must not be empty"));
        }
        if series.label.chars().count() > 80 || series.color.chars().count() > 32 {
            return Err(format!(
                "series[{index}] label must not exceed 80 characters and color 32 characters"
            ));
        }
        if series.color.trim().is_empty() {
            return Err(format!("series[{index}].color must not be empty"));
        }
        if !fields.insert(series.field.as_str()) {
            return Err(format!("duplicate series field: {}", series.field));
        }
    }

    let mut categories = Vec::with_capacity(document.table.rows.len());
    let mut values = Vec::with_capacity(document.table.rows.len());
    let mut identifiers = HashSet::new();
    let scale = 10_u64.pow(document.axes.y.format.decimals) as f64;
    if document
        .axes
        .y
        .max
        .is_some_and(|maximum| maximum * scale > u64::MAX as f64)
    {
        return Err("axes.y.max exceeds Ratatui BarChart's scaled u64 range".to_string());
    }

    for (row_index, row) in document.table.rows.iter().enumerate() {
        if row.len() < 2 {
            return Err(format!(
                "table.rows[{row_index}] must contain at least two fields"
            ));
        }
        for (field, value) in row {
            if !is_scalar(value) {
                return Err(format!(
                    "table.rows[{row_index}].{field} must be a scalar value"
                ));
            }
        }
        let category_value = row.get(&document.axes.x.field).ok_or_else(|| {
            format!(
                "table.rows[{row_index}] is missing category field {}",
                document.axes.x.field
            )
        })?;
        let category = scalar_label(category_value).ok_or_else(|| {
            format!(
                "table.rows[{row_index}].{} must be a string or number",
                document.axes.x.field
            )
        })?;
        categories.push(category);

        if let Some(id_field) = document.table.id_field.as_deref() {
            let identifier = row.get(id_field).and_then(scalar_label).ok_or_else(|| {
                format!("table.rows[{row_index}].{id_field} must be a string or number")
            })?;
            if !identifiers.insert(identifier.clone()) {
                return Err(format!("duplicate table id in {id_field}: {identifier}"));
            }
        }

        let mut row_values = Vec::with_capacity(document.series.len());
        for series in &document.series {
            let value = row
                .get(&series.field)
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    format!(
                        "table.rows[{row_index}].{} must be a finite number",
                        series.field
                    )
                })?;
            if !value.is_finite() || value < 0.0 {
                return Err(format!(
                    "table.rows[{row_index}].{} must be finite and non-negative",
                    series.field
                ));
            }
            if value * scale > u64::MAX as f64 {
                return Err(format!(
                    "table.rows[{row_index}].{} exceeds Ratatui BarChart's scaled u64 range",
                    series.field
                ));
            }
            row_values.push(value);
        }
        values.push(row_values);
    }

    Ok((categories, values))
}

fn scalar_label(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn default_true() -> bool {
    true
}

fn default_show_values() -> ShowValues {
    ShowValues::Selected
}

fn default_bar_width() -> u16 {
    3
}

fn default_bar_gap() -> u16 {
    1
}

fn default_group_gap() -> u16 {
    2
}

fn default_color() -> String {
    "cyan".to_string()
}
