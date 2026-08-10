use std::collections::{BTreeSet, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::app::{App, node_value};
use ft_canvas_runtime::{CanvasClient, LaunchConfig};

const MAX_CONTEXT_NODES: usize = 120;
const MAX_CONTEXT_EDGES: usize = 480;

pub fn send_selection_update(app: &App, client: &CanvasClient) -> Result<(), String> {
    let count = app.selected.len();
    let text = if count == 0 {
        "The DAG node selection was cleared.".to_string()
    } else {
        format!("The user selected {count} DAG node(s).")
    };
    client.send_selection("selected DAG nodes", text, app.selection_value())
}

pub fn send_context(app: &App, client: &CanvasClient) -> Result<(), String> {
    let data = context_value(app)?;
    let text = context_summary(app, &data);
    client.send_context("DAG context", text, data)
}

pub fn send_analysis_action(app: &App, client: &CanvasClient) -> Result<(), String> {
    let data = context_value(app)?;
    let summary = context_summary(app, &data);
    let prompt = format!(
        "Analyze this directed acyclic graph. Explain the dependency flow, critical paths, fan-in/fan-out, bottlenecks, and structural risks. Use only the supplied nodes and edges. {summary}"
    );
    client.send_action("analyze DAG", prompt, data)
}

pub fn export_artifact(
    app: &App,
    client: &CanvasClient,
    launch: &LaunchConfig,
) -> Result<(), String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "no DAG is available to export".to_string())?;
    let data = context_value(app)?;
    let bytes = serde_json::to_vec_pretty(&data)
        .map_err(|error| format!("cannot encode DAG artifact: {error}"))?;
    let filename = format!(
        "{}-dag-{}.json",
        safe_filename(&dataset.document.title),
        now_millis()
    );
    let path = launch.runtime_dir.join(filename);
    std::fs::write(&path, bytes)
        .map_err(|error| format!("cannot write DAG artifact {}: {error}", path.display()))?;
    client.send_artifact(
        "DAG data",
        &path,
        "Exported the selected DAG neighborhood or visible viewport.".to_string(),
    )
}

fn context_value(app: &App) -> Result<Value, String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "no DAG data is loaded".to_string())?;
    let mut indices: BTreeSet<usize> = if app.selected.is_empty() {
        app.visible_node_indices().into_iter().collect()
    } else {
        app.selected.clone()
    };
    let selection_scope = !app.selected.is_empty();
    if selection_scope {
        for edge in &dataset.document.edges {
            let from = dataset.node_index[&edge.from];
            let to = dataset.node_index[&edge.to];
            if app.selected.contains(&from) || app.selected.contains(&to) {
                indices.insert(from);
                indices.insert(to);
            }
        }
    }
    if indices.is_empty() {
        indices.extend(dataset.topological_order.iter().copied());
    }

    let truncated_nodes = indices.len() > MAX_CONTEXT_NODES;
    let kept_indices: BTreeSet<usize> = indices.into_iter().take(MAX_CONTEXT_NODES).collect();
    let kept_ids: HashSet<&str> = kept_indices
        .iter()
        .map(|index| dataset.document.nodes[*index].id.as_str())
        .collect();
    let nodes = kept_indices
        .iter()
        .map(|index| node_value(dataset, *index))
        .collect::<Vec<_>>();
    let matching_edges = dataset
        .document
        .edges
        .iter()
        .filter(|edge| kept_ids.contains(edge.from.as_str()) && kept_ids.contains(edge.to.as_str()))
        .collect::<Vec<_>>();
    let truncated_edges = matching_edges.len() > MAX_CONTEXT_EDGES;
    let edges = matching_edges
        .into_iter()
        .take(MAX_CONTEXT_EDGES)
        .collect::<Vec<_>>();

    Ok(json!({
        "title": dataset.document.title,
        "subtitle": dataset.document.subtitle,
        "direction": dataset.document.layout.direction.as_str(),
        "scope": if selection_scope { "selection-neighborhood" } else { "visible-viewport" },
        "nodes": nodes,
        "edges": edges,
        "truncated": truncated_nodes || truncated_edges,
        "source": dataset.source,
        "metadata": dataset.document.metadata
    }))
}

fn context_summary(app: &App, data: &Value) -> String {
    let title = app
        .dataset
        .as_ref()
        .map(|dataset| dataset.document.title.as_str())
        .unwrap_or("DAG");
    let nodes = data
        .get("nodes")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let edges = data
        .get("edges")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    format!("DAG `{title}` context contains {nodes} nodes and {edges} edges.")
}

fn safe_filename(value: &str) -> String {
    let name: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if name.trim_matches('-').is_empty() {
        "dag".to_string()
    } else {
        name
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
