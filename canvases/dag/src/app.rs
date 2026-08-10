use std::collections::{BTreeSet, HashSet};

use ratatui::layout::Rect;
use serde_json::{Value, json};

use crate::config::{RendererConfig, RendererSource};
use crate::data::DagDataset;
use crate::layout::{GraphLayout, WorldRect};

const MAX_SELECTED_NODES: usize = 128;

#[derive(Debug, Clone, Copy)]
pub struct ViewInfo {
    pub area: Rect,
    pub offset_x: i32,
    pub offset_y: i32,
}

impl ViewInfo {
    pub fn empty() -> Self {
        Self {
            area: Rect::new(0, 0, 0, 0),
            offset_x: 0,
            offset_y: 0,
        }
    }

    pub fn world_rect(self) -> WorldRect {
        WorldRect {
            x: self.offset_x,
            y: self.offset_y,
            width: i32::from(self.area.width),
            height: i32::from(self.area.height),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DragState {
    pub last_column: u16,
    pub last_row: u16,
    pub pressed_node: Option<usize>,
    pub moved: bool,
}

pub struct App {
    pub dataset: Option<DagDataset>,
    pub graph: GraphLayout,
    pub view: ViewInfo,
    pub offset_x: i32,
    pub offset_y: i32,
    pub selected: BTreeSet<usize>,
    pub active_node: usize,
    pub drag: Option<DragState>,
    pub status: String,
    pub loading: bool,
    pub generation: u64,
    pub loaded_generation: Option<u64>,
    pub generation_request_id: Option<String>,
    pub initial_config: Option<Value>,
    pub init_received: bool,
    pub dirty: bool,
    pub state_dirty: bool,
    pub should_exit: bool,
}

impl App {
    pub fn new(initial_config: Value) -> Self {
        Self {
            dataset: None,
            graph: GraphLayout::default(),
            view: ViewInfo::empty(),
            offset_x: 0,
            offset_y: 0,
            selected: BTreeSet::new(),
            active_node: 0,
            drag: None,
            status: "Loading DAG...".to_string(),
            loading: true,
            generation: 0,
            loaded_generation: None,
            generation_request_id: None,
            initial_config: Some(initial_config),
            init_received: false,
            dirty: true,
            state_dirty: false,
            should_exit: false,
        }
    }

    pub fn apply_dataset(&mut self, dataset: DagDataset) {
        let selected_ids: HashSet<String> = self
            .dataset
            .as_ref()
            .map(|current| {
                self.selected
                    .iter()
                    .filter_map(|index| current.document.nodes.get(*index))
                    .map(|node| node.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let active_id = self
            .dataset
            .as_ref()
            .and_then(|current| current.document.nodes.get(self.active_node))
            .map(|node| node.id.clone());

        self.graph = GraphLayout::from_dataset(&dataset);
        self.selected = dataset
            .document
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| selected_ids.contains(&node.id).then_some(index))
            .collect();
        self.active_node = active_id
            .as_ref()
            .and_then(|id| dataset.node_index.get(id))
            .copied()
            .or_else(|| dataset.topological_order.first().copied())
            .unwrap_or(0);
        self.dataset = Some(dataset);
        self.offset_x = 0;
        self.offset_y = 0;
        self.drag = None;
        self.loading = false;
        self.clamp_view();
        self.status = self
            .dataset
            .as_ref()
            .map(|dataset| {
                format!(
                    "{} nodes | {} edges | drag pan | click select | a attach | Enter analyze | e export",
                    dataset.document.nodes.len(),
                    dataset.document.edges.len()
                )
            })
            .unwrap_or_default();
        self.dirty = true;
        self.state_dirty = true;
    }

    pub fn clamp_view(&mut self) {
        let max_x = (self.graph.width - i32::from(self.view.area.width)).max(0);
        let max_y = (self.graph.height - i32::from(self.view.area.height)).max(0);
        self.offset_x = self.offset_x.clamp(0, max_x);
        self.offset_y = self.offset_y.clamp(0, max_y);
    }

    pub fn pan(&mut self, dx: i32, dy: i32) {
        let previous = (self.offset_x, self.offset_y);
        self.offset_x = self.offset_x.saturating_add(dx);
        self.offset_y = self.offset_y.saturating_add(dy);
        self.clamp_view();
        if previous != (self.offset_x, self.offset_y) {
            self.dirty = true;
            self.state_dirty = true;
        }
    }

    pub fn reset_view(&mut self) {
        if self.offset_x != 0 || self.offset_y != 0 {
            self.offset_x = 0;
            self.offset_y = 0;
            self.dirty = true;
            self.state_dirty = true;
        }
    }

    pub fn toggle_node(&mut self, index: usize) -> bool {
        let Some(dataset) = self.dataset.as_ref() else {
            return false;
        };
        if index >= dataset.document.nodes.len() {
            return false;
        }
        self.active_node = index;
        if self.selected.remove(&index) {
            self.dirty = true;
            self.state_dirty = true;
            return true;
        }
        if self.selected.len() >= MAX_SELECTED_NODES {
            self.status = format!("Selection is limited to {MAX_SELECTED_NODES} DAG nodes");
            self.dirty = true;
            return false;
        }
        self.selected.insert(index);
        self.dirty = true;
        self.state_dirty = true;
        true
    }

    pub fn clear_selection(&mut self) {
        if !self.selected.is_empty() {
            self.selected.clear();
            self.dirty = true;
            self.state_dirty = true;
        }
    }

    pub fn cycle_active(&mut self, backwards: bool) {
        let Some(dataset) = self.dataset.as_ref() else {
            return;
        };
        let order = &dataset.topological_order;
        if order.is_empty() {
            return;
        }
        let position = order
            .iter()
            .position(|index| *index == self.active_node)
            .unwrap_or(0);
        let next = if backwards {
            position.checked_sub(1).unwrap_or(order.len() - 1)
        } else {
            (position + 1) % order.len()
        };
        self.active_node = order[next];
        self.ensure_active_visible();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn ensure_active_visible(&mut self) {
        let Some(rect) = self.graph.nodes.get(self.active_node).copied() else {
            return;
        };
        let viewport_width = i32::from(self.view.area.width);
        let viewport_height = i32::from(self.view.area.height);
        if rect.x < self.offset_x {
            self.offset_x = rect.x;
        } else if rect.right() > self.offset_x + viewport_width {
            self.offset_x = rect.right() - viewport_width;
        }
        if rect.y < self.offset_y {
            self.offset_y = rect.y;
        } else if rect.bottom() > self.offset_y + viewport_height {
            self.offset_y = rect.bottom() - viewport_height;
        }
        self.clamp_view();
    }

    pub fn selection_value(&self) -> Value {
        let Some(dataset) = self.dataset.as_ref() else {
            return Value::Null;
        };
        let nodes = self
            .selected
            .iter()
            .map(|index| node_value(dataset, *index))
            .collect::<Vec<_>>();
        let selected_ids: HashSet<&str> = self
            .selected
            .iter()
            .filter_map(|index| dataset.document.nodes.get(*index))
            .map(|node| node.id.as_str())
            .collect();
        let edges = dataset
            .document
            .edges
            .iter()
            .filter(|edge| {
                selected_ids.contains(edge.from.as_str()) || selected_ids.contains(edge.to.as_str())
            })
            .collect::<Vec<_>>();
        json!({
            "title": dataset.document.title,
            "count": nodes.len(),
            "nodes": nodes,
            "adjacentEdges": edges
        })
    }

    pub fn state_value(&self, key: Option<&str>) -> Result<Value, String> {
        let Some(dataset) = self.dataset.as_ref() else {
            return Ok(json!({ "status": self.status, "loading": self.loading }));
        };
        let viewport = viewport_value(self);
        let selection = self.selection_value();
        let active_node = dataset
            .document
            .nodes
            .get(self.active_node)
            .map(|_| node_value(dataset, self.active_node))
            .unwrap_or(Value::Null);
        let graph = json!({
            "title": dataset.document.title,
            "direction": dataset.document.layout.direction.as_str(),
            "nodeCount": dataset.document.nodes.len(),
            "edgeCount": dataset.document.edges.len(),
            "source": dataset.source,
            "input": source_value(&dataset.config),
            "worldWidth": self.graph.width,
            "worldHeight": self.graph.height
        });
        let all = json!({
            "graph": graph.clone(),
            "viewport": viewport.clone(),
            "selection": selection.clone(),
            "activeNode": active_node.clone(),
            "loading": self.loading
        });
        match key {
            None => Ok(all),
            Some("graph") => Ok(graph),
            Some("viewport") => Ok(viewport),
            Some("selection") => Ok(selection),
            Some("activeNode") => Ok(active_node),
            Some(other) => Err(format!("unknown state key: {other}")),
        }
    }

    pub fn ready_title(&self) -> Option<String> {
        self.dataset
            .as_ref()
            .map(|dataset| dataset.document.title.clone())
    }

    pub fn visible_node_indices(&self) -> Vec<usize> {
        let viewport = self.view.world_rect();
        self.graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, rect)| rect.intersects(viewport).then_some(index))
            .collect()
    }
}

pub fn node_value(dataset: &DagDataset, index: usize) -> Value {
    let node = &dataset.document.nodes[index];
    json!({
        "index": index,
        "rank": dataset.ranks[index],
        "id": node.id,
        "label": node.label,
        "description": node.description,
        "color": node.color,
        "metadata": node.metadata
    })
}

fn viewport_value(app: &App) -> Value {
    json!({
        "offsetX": app.view.offset_x,
        "offsetY": app.view.offset_y,
        "width": app.view.area.width,
        "height": app.view.area.height,
        "worldWidth": app.graph.width,
        "worldHeight": app.graph.height,
        "overflowX": app.graph.width > i32::from(app.view.area.width),
        "overflowY": app.graph.height > i32::from(app.view.area.height)
    })
}

fn source_value(config: &RendererConfig) -> Value {
    match &config.source {
        RendererSource::Inline(_) => json!({ "type": "inline" }),
        RendererSource::File(path) => json!({ "type": "file", "dataFile": path }),
    }
}
