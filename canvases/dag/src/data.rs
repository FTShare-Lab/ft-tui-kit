use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::BufReader;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::{RendererConfig, RendererSource};

const MAX_DATA_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_NODES: usize = 500;
const MAX_EDGES: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DagDocument {
    pub schema_version: u8,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub layout: LayoutSpec,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_node_color")]
    pub color: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    #[default]
    LeftToRight,
    TopToBottom,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LeftToRight => "left-to-right",
            Self::TopToBottom => "top-to-bottom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutSpec {
    #[serde(default)]
    pub direction: Direction,
    #[serde(default = "default_min_node_width")]
    pub min_node_width: u16,
    #[serde(default = "default_max_node_width")]
    pub max_node_width: u16,
    #[serde(default = "default_min_node_height")]
    pub min_node_height: u16,
    #[serde(default = "default_max_node_height")]
    pub max_node_height: u16,
    #[serde(default = "default_layer_gap")]
    pub layer_gap: u16,
    #[serde(default = "default_node_gap")]
    pub node_gap: u16,
    #[serde(default = "default_padding")]
    pub padding: u16,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            direction: Direction::default(),
            min_node_width: default_min_node_width(),
            max_node_width: default_max_node_width(),
            min_node_height: default_min_node_height(),
            max_node_height: default_max_node_height(),
            layer_gap: default_layer_gap(),
            node_gap: default_node_gap(),
            padding: default_padding(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DagDataset {
    pub config: RendererConfig,
    pub document: DagDocument,
    pub node_index: HashMap<String, usize>,
    pub ranks: Vec<usize>,
    pub topological_order: Vec<usize>,
    pub source: String,
}

pub fn load_dag_data(config: RendererConfig) -> Result<DagDataset, String> {
    let (raw, source) = match &config.source {
        RendererSource::Inline(value) => (value.clone(), "inline config".to_string()),
        RendererSource::File(path) => {
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
            let file = File::open(path)
                .map_err(|error| format!("cannot open data_file {}: {error}", path.display()))?;
            let value = serde_json::from_reader(BufReader::new(file))
                .map_err(|error| format!("data_file contains invalid JSON: {error}"))?;
            (value, format!("file:{}", path.display()))
        }
    };

    validate_required_fields(&raw)?;
    let document: DagDocument = serde_json::from_value(raw)
        .map_err(|error| format!("DAG document contains an invalid field: {error}"))?;
    let (node_index, ranks, topological_order) = validate_document(&document)?;
    let source = document
        .metadata
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or(source);

    Ok(DagDataset {
        config,
        document,
        node_index,
        ranks,
        topological_order,
        source,
    })
}

fn validate_required_fields(value: &Value) -> Result<(), String> {
    let document = require_object(value, "document")?;
    require_field(document, "schemaVersion", "")?;
    require_field(document, "title", "")?;
    let nodes = require_array(require_field(document, "nodes", "")?, "nodes")?;
    for (index, value) in nodes.iter().enumerate() {
        let path = format!("nodes[{index}]");
        let node = require_object(value, &path)?;
        require_field(node, "id", &path)?;
        require_field(node, "label", &path)?;
    }
    let edges = require_array(require_field(document, "edges", "")?, "edges")?;
    for (index, value) in edges.iter().enumerate() {
        let path = format!("edges[{index}]");
        let edge = require_object(value, &path)?;
        require_field(edge, "from", &path)?;
        require_field(edge, "to", &path)?;
    }
    Ok(())
}

fn validate_document(
    document: &DagDocument,
) -> Result<(HashMap<String, usize>, Vec<usize>, Vec<usize>), String> {
    if document.schema_version != 1 {
        return Err(format!(
            "unsupported schemaVersion {}, expected 1",
            document.schema_version
        ));
    }
    validate_text("title", &document.title, 160)?;
    if let Some(subtitle) = &document.subtitle
        && subtitle.chars().count() > 240
    {
        return Err("subtitle must not exceed 240 characters".to_string());
    }
    if document.nodes.is_empty() || document.nodes.len() > MAX_NODES {
        return Err(format!("nodes must contain 1 to {MAX_NODES} entries"));
    }
    if document.edges.len() > MAX_EDGES {
        return Err(format!("edges must contain at most {MAX_EDGES} entries"));
    }
    validate_layout(&document.layout)?;
    validate_metadata("metadata", &document.metadata)?;

    let mut node_index = HashMap::with_capacity(document.nodes.len());
    for (index, node) in document.nodes.iter().enumerate() {
        validate_text(&format!("nodes[{index}].id"), &node.id, 128)?;
        validate_text(&format!("nodes[{index}].label"), &node.label, 160)?;
        if node
            .description
            .as_ref()
            .is_some_and(|description| description.chars().count() > 500)
        {
            return Err(format!(
                "nodes[{index}].description must not exceed 500 characters"
            ));
        }
        validate_text(&format!("nodes[{index}].color"), &node.color, 32)?;
        validate_metadata(&format!("nodes[{index}].metadata"), &node.metadata)?;
        if node_index.insert(node.id.clone(), index).is_some() {
            return Err(format!("duplicate node id: {}", node.id));
        }
    }

    let mut adjacency = vec![Vec::new(); document.nodes.len()];
    let mut indegree = vec![0_usize; document.nodes.len()];
    let mut edge_keys = HashSet::new();
    for (index, edge) in document.edges.iter().enumerate() {
        validate_text(&format!("edges[{index}].from"), &edge.from, 128)?;
        validate_text(&format!("edges[{index}].to"), &edge.to, 128)?;
        if edge
            .label
            .as_ref()
            .is_some_and(|label| label.chars().count() > 120)
        {
            return Err(format!(
                "edges[{index}].label must not exceed 120 characters"
            ));
        }
        if edge
            .label
            .as_ref()
            .is_some_and(|label| label.contains('\n') || label.contains('\r'))
        {
            return Err(format!("edges[{index}].label must be a single line"));
        }
        if let Some(color) = edge.color.as_deref() {
            validate_text(&format!("edges[{index}].color"), color, 32)?;
        }
        validate_metadata(&format!("edges[{index}].metadata"), &edge.metadata)?;
        let from = node_index.get(&edge.from).copied().ok_or_else(|| {
            format!(
                "edges[{index}].from references unknown node id `{}`",
                edge.from
            )
        })?;
        let to = node_index
            .get(&edge.to)
            .copied()
            .ok_or_else(|| format!("edges[{index}].to references unknown node id `{}`", edge.to))?;
        if from == to {
            return Err(format!("edges[{index}] is a self-edge on `{}`", edge.from));
        }
        if !edge_keys.insert((from, to)) {
            return Err(format!(
                "duplicate edge from `{}` to `{}`",
                edge.from, edge.to
            ));
        }
        adjacency[from].push(to);
        indegree[to] += 1;
    }

    let mut queue: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut topological_order = Vec::with_capacity(document.nodes.len());
    let mut ranks = vec![0_usize; document.nodes.len()];
    while let Some(node) = queue.pop_front() {
        topological_order.push(node);
        for &target in &adjacency[node] {
            ranks[target] = ranks[target].max(ranks[node] + 1);
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if topological_order.len() != document.nodes.len() {
        let unresolved = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, degree)| {
                (*degree > 0).then_some(document.nodes[index].id.as_str())
            })
            .take(8)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "graph contains a cycle; DAG nodes still in the cycle: {unresolved}"
        ));
    }

    Ok((node_index, ranks, topological_order))
}

fn validate_layout(layout: &LayoutSpec) -> Result<(), String> {
    if !(8..=48).contains(&layout.min_node_width) {
        return Err("layout.minNodeWidth must be between 8 and 48".to_string());
    }
    if !(12..=64).contains(&layout.max_node_width) || layout.max_node_width < layout.min_node_width
    {
        return Err(
            "layout.maxNodeWidth must be between 12 and 64 and not less than minNodeWidth"
                .to_string(),
        );
    }
    if !(3..=8).contains(&layout.min_node_height) {
        return Err("layout.minNodeHeight must be between 3 and 8".to_string());
    }
    if !(3..=12).contains(&layout.max_node_height)
        || layout.max_node_height < layout.min_node_height
    {
        return Err(
            "layout.maxNodeHeight must be between 3 and 12 and not less than minNodeHeight"
                .to_string(),
        );
    }
    if !(4..=24).contains(&layout.layer_gap) {
        return Err("layout.layerGap must be between 4 and 24".to_string());
    }
    if !(1..=12).contains(&layout.node_gap) {
        return Err("layout.nodeGap must be between 1 and 12".to_string());
    }
    if layout.padding > 8 {
        return Err("layout.padding must be between 0 and 8".to_string());
    }
    Ok(())
}

fn validate_text(path: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{path} must not be empty"));
    }
    if value.chars().count() > maximum {
        return Err(format!("{path} must not exceed {maximum} characters"));
    }
    Ok(())
}

fn validate_metadata(path: &str, metadata: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in metadata {
        if !matches!(
            value,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
        ) {
            return Err(format!("{path}.{key} must be a scalar value"));
        }
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

fn default_node_color() -> String {
    "cyan".to_string()
}

fn default_min_node_width() -> u16 {
    12
}

fn default_max_node_width() -> u16 {
    48
}

fn default_min_node_height() -> u16 {
    3
}

fn default_max_node_height() -> u16 {
    8
}

fn default_layer_gap() -> u16 {
    8
}

fn default_node_gap() -> u16 {
    2
}

fn default_padding() -> u16 {
    2
}
