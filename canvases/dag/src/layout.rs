use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::data::{DagDataset, Direction, LayoutSpec, NodeSpec};
use crate::routing::{EdgeGeometry, OutgoingRoutes, build_edge_geometries};

#[derive(Debug, Clone, Copy, Default)]
pub struct WorldRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl WorldRect {
    pub fn right(self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn center_x_cell(self) -> i32 {
        self.x + self.width / 2
    }

    pub fn center_y_cell(self) -> i32 {
        self.y + self.height / 2
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphLayout {
    pub nodes: Vec<WorldRect>,
    pub edges: Vec<EdgeGeometry>,
    pub width: i32,
    pub height: i32,
}

impl GraphLayout {
    pub fn from_dataset(dataset: &DagDataset) -> Self {
        let spec = &dataset.document.layout;
        let layer_count = dataset.ranks.iter().copied().max().unwrap_or(0) + 1;
        let mut layers = vec![Vec::new(); layer_count];
        for &node_index in &dataset.topological_order {
            layers[dataset.ranks[node_index]].push(node_index);
        }
        let routes = OutgoingRoutes::from_dataset(dataset);
        let sizes = dataset
            .document
            .nodes
            .iter()
            .enumerate()
            .map(|(node_index, node)| adaptive_node_size(node, spec, routes.count_for(node_index)))
            .collect::<Vec<_>>();

        let mut graph = match spec.direction {
            Direction::LeftToRight => layout_left_to_right(dataset, layers, sizes, &routes),
            Direction::TopToBottom => layout_top_to_bottom(dataset, layers, sizes, &routes),
        };
        graph.edges = build_edge_geometries(dataset, &graph.nodes, &routes);
        graph
    }
}

fn layout_left_to_right(
    dataset: &DagDataset,
    layers: Vec<Vec<usize>>,
    sizes: Vec<(i32, i32)>,
    routes: &OutgoingRoutes,
) -> GraphLayout {
    let spec = &dataset.document.layout;
    let layer_count = layers.len();
    let layer_widths = layers
        .iter()
        .map(|nodes| nodes.iter().map(|index| sizes[*index].0).max().unwrap_or(1))
        .collect::<Vec<_>>();
    let layer_heights = layers
        .iter()
        .map(|nodes| layer_extent(nodes, &sizes, i32::from(spec.node_gap), false))
        .collect::<Vec<_>>();
    let content_height = layer_heights.iter().copied().max().unwrap_or(1);
    let mut layer_gaps = vec![i32::from(spec.layer_gap); layer_count.saturating_sub(1)];
    for (edge_index, edge) in dataset.document.edges.iter().enumerate() {
        let from = dataset.node_index[&edge.from];
        let to = dataset.node_index[&edge.to];
        let from_rank = dataset.ranks[from];
        let to_rank = dataset.ranks[to];
        if to_rank > from_rank {
            let label_space = edge
                .label
                .as_deref()
                .map_or(0, text_width)
                .saturating_add(6) as i32;
            let routed_space = label_space + routes.slots[edge_index] as i32;
            let target_gap = to_rank - 1;
            layer_gaps[target_gap] = layer_gaps[target_gap].max(routed_space);
        }
    }

    let padding = i32::from(spec.padding);
    let mut layer_x = vec![padding; layer_count];
    for rank in 1..layer_count {
        layer_x[rank] = layer_x[rank - 1] + layer_widths[rank - 1] + layer_gaps[rank - 1];
    }
    let mut nodes = vec![WorldRect::default(); dataset.document.nodes.len()];
    for (rank, indices) in layers.iter().enumerate() {
        let mut y = padding + (content_height - layer_heights[rank]) / 2;
        for &index in indices {
            let (width, height) = sizes[index];
            nodes[index] = WorldRect {
                x: layer_x[rank] + (layer_widths[rank] - width) / 2,
                y,
                width,
                height,
            };
            y += height + i32::from(spec.node_gap);
        }
    }
    let graph_width = layer_x.last().copied().unwrap_or(padding)
        + layer_widths.last().copied().unwrap_or(1)
        + padding;

    GraphLayout {
        nodes,
        edges: Vec::new(),
        width: graph_width.max(1),
        height: (padding * 2 + content_height).max(1),
    }
}

fn layout_top_to_bottom(
    dataset: &DagDataset,
    layers: Vec<Vec<usize>>,
    sizes: Vec<(i32, i32)>,
    routes: &OutgoingRoutes,
) -> GraphLayout {
    let spec = &dataset.document.layout;
    let layer_count = layers.len();
    let layer_widths = layers
        .iter()
        .map(|nodes| layer_extent(nodes, &sizes, i32::from(spec.node_gap), true))
        .collect::<Vec<_>>();
    let layer_heights = layers
        .iter()
        .map(|nodes| nodes.iter().map(|index| sizes[*index].1).max().unwrap_or(1))
        .collect::<Vec<_>>();
    let content_width = layer_widths.iter().copied().max().unwrap_or(1);
    let label_margin = dataset
        .document
        .edges
        .iter()
        .filter_map(|edge| edge.label.as_deref())
        .map(text_width)
        .max()
        .unwrap_or(0) as i32
        / 2;
    let side_margin = label_margin.saturating_add(1);
    let padding = i32::from(spec.padding);
    let mut layer_gaps = vec![i32::from(spec.layer_gap); layer_count.saturating_sub(1)];
    for (edge_index, edge) in dataset.document.edges.iter().enumerate() {
        let from = dataset.node_index[&edge.from];
        let to = dataset.node_index[&edge.to];
        let from_rank = dataset.ranks[from];
        let to_rank = dataset.ranks[to];
        if to_rank > from_rank {
            let routed_space = routes.slots[edge_index] as i32 + 4;
            let target_gap = to_rank - 1;
            layer_gaps[target_gap] = layer_gaps[target_gap].max(routed_space);
        }
    }
    let mut layer_y = vec![padding; layer_count];
    for rank in 1..layer_count {
        layer_y[rank] = layer_y[rank - 1] + layer_heights[rank - 1] + layer_gaps[rank - 1];
    }
    let mut nodes = vec![WorldRect::default(); dataset.document.nodes.len()];
    for (rank, indices) in layers.iter().enumerate() {
        let mut x = padding + side_margin + (content_width - layer_widths[rank]) / 2;
        for &index in indices {
            let (width, height) = sizes[index];
            nodes[index] = WorldRect {
                x,
                y: layer_y[rank] + (layer_heights[rank] - height) / 2,
                width,
                height,
            };
            x += width + i32::from(spec.node_gap);
        }
    }
    let graph_height = layer_y.last().copied().unwrap_or(padding)
        + layer_heights.last().copied().unwrap_or(1)
        + padding;

    GraphLayout {
        nodes,
        edges: Vec::new(),
        width: (padding * 2 + side_margin * 2 + content_width).max(1),
        height: graph_height.max(1),
    }
}

fn layer_extent(nodes: &[usize], sizes: &[(i32, i32)], gap: i32, horizontal: bool) -> i32 {
    let content = nodes
        .iter()
        .map(|index| {
            if horizontal {
                sizes[*index].0
            } else {
                sizes[*index].1
            }
        })
        .sum::<i32>();
    content + nodes.len().saturating_sub(1) as i32 * gap
}

fn adaptive_node_size(node: &NodeSpec, spec: &LayoutSpec, outgoing_count: usize) -> (i32, i32) {
    let preferred_width = text_width(&node.label)
        .max(text_width(&node.id).min(30))
        .saturating_add(4);
    let route_width = match spec.direction {
        Direction::LeftToRight => 0,
        Direction::TopToBottom => outgoing_count.saturating_add(2) / 2 + 1,
    };
    let width = preferred_width
        .max(route_width)
        .max(usize::from(spec.min_node_width))
        .min(usize::from(spec.max_node_width));
    let inner_width = width.saturating_sub(2).max(1);
    let label_lines = wrap_text(&node.label, inner_width).len();
    let description_lines = node
        .description
        .as_deref()
        .map_or(0, |description| wrap_text(description, inner_width).len());
    let content_lines = label_lines + 1 + description_lines;
    let route_height = match spec.direction {
        Direction::LeftToRight => outgoing_count.saturating_add(4) / 4 + 1,
        Direction::TopToBottom => 0,
    };
    let height = content_lines
        .saturating_add(2)
        .max(route_height)
        .max(usize::from(spec.min_node_height))
        .min(usize::from(spec.max_node_height));
    (width as i32, height as i32)
}

pub fn text_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub fn truncate_text(value: &str, maximum: usize) -> String {
    if maximum == 0 {
        return String::new();
    }
    if text_width(value) <= maximum {
        return value.to_string();
    }
    let target = maximum.saturating_sub(1);
    let mut output = String::new();
    let mut width = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > target {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output.push('~');
    output
}

pub fn wrap_text(value: &str, maximum: usize) -> Vec<String> {
    if maximum == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for source_line in value.split('\n') {
        let mut current = String::new();
        let mut width = 0;
        for character in source_line.chars() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if !current.is_empty() && width + character_width > maximum {
                lines.push(current);
                current = String::new();
                width = 0;
            }
            current.push(character);
            width += character_width;
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
