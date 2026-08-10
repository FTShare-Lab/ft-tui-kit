use crate::data::{DagDataset, Direction};
use crate::layout::WorldRect;

#[derive(Debug, Clone)]
pub struct EdgeGeometry {
    pub segments: Vec<(f64, f64, f64, f64)>,
    pub label_center: Option<(f64, f64)>,
    pub arrow_x: i32,
    pub arrow_y: i32,
    pub arrow: &'static str,
}

#[derive(Debug, Clone)]
pub struct OutgoingRoutes {
    pub slots: Vec<usize>,
    counts: Vec<usize>,
}

impl OutgoingRoutes {
    pub fn from_dataset(dataset: &DagDataset) -> Self {
        let mut edges_by_source = vec![Vec::new(); dataset.document.nodes.len()];
        let mut topological_position = vec![0_usize; dataset.document.nodes.len()];
        for (position, &node) in dataset.topological_order.iter().enumerate() {
            topological_position[node] = position;
        }
        for (edge_index, edge) in dataset.document.edges.iter().enumerate() {
            let source = dataset.node_index[&edge.from];
            edges_by_source[source].push(edge_index);
        }

        let mut slots = vec![0_usize; dataset.document.edges.len()];
        let mut counts = vec![0_usize; dataset.document.nodes.len()];
        for (source, edge_indices) in edges_by_source.iter_mut().enumerate() {
            edge_indices.sort_by_key(|edge_index| {
                let edge = &dataset.document.edges[*edge_index];
                let target = dataset.node_index[&edge.to];
                (
                    dataset.ranks[target],
                    topological_position[target],
                    *edge_index,
                )
            });
            counts[source] = edge_indices.len();
            for (slot, &edge_index) in edge_indices.iter().enumerate() {
                slots[edge_index] = slot;
            }
        }
        Self { slots, counts }
    }

    pub fn count_for(&self, node: usize) -> usize {
        self.counts[node]
    }
}

pub fn build_edge_geometries(
    dataset: &DagDataset,
    nodes: &[WorldRect],
    routes: &OutgoingRoutes,
) -> Vec<EdgeGeometry> {
    dataset
        .document
        .edges
        .iter()
        .enumerate()
        .map(|(edge_index, edge)| {
            let source = dataset.node_index[&edge.from];
            let target = dataset.node_index[&edge.to];
            let slot = routes.slots[edge_index];
            let count = routes.counts[source].max(1);
            route_edge(
                nodes[source],
                nodes[target],
                dataset.document.layout.direction,
                slot,
                count,
                edge.label.as_deref(),
            )
        })
        .collect()
}

fn route_edge(
    from: WorldRect,
    to: WorldRect,
    direction: Direction,
    slot: usize,
    count: usize,
    label: Option<&str>,
) -> EdgeGeometry {
    match direction {
        Direction::LeftToRight => {
            let x1 = f64::from(from.right());
            let y1 = distributed_port(from.y, from.height, slot, count);
            let arrow_x = to.x - 1;
            let y2 = to.center_y_cell();
            let label_width = label.map_or(0, unicode_width::UnicodeWidthStr::width) as i32;
            let turn_x = (arrow_x - label_width - 4 - slot as i32).max(from.right() + 1);
            EdgeGeometry {
                segments: vec![
                    (x1, y1, f64::from(turn_x), y1),
                    (f64::from(turn_x), y1, f64::from(turn_x), f64::from(y2)),
                    (
                        f64::from(turn_x),
                        f64::from(y2),
                        f64::from(arrow_x),
                        f64::from(y2),
                    ),
                ],
                label_center: label
                    .is_some()
                    .then_some((f64::from(turn_x + arrow_x) / 2.0, f64::from(y2))),
                arrow_x,
                arrow_y: y2,
                arrow: "→",
            }
        }
        Direction::TopToBottom => {
            let x1 = distributed_port(from.x, from.width, slot, count);
            let y1 = f64::from(from.bottom());
            let x2 = to.center_x_cell();
            let arrow_y = to.y - 1;
            let turn_y = (arrow_y - 3 - slot as i32).max(from.bottom() + 1);
            EdgeGeometry {
                segments: vec![
                    (x1, y1, x1, f64::from(turn_y)),
                    (x1, f64::from(turn_y), f64::from(x2), f64::from(turn_y)),
                    (
                        f64::from(x2),
                        f64::from(turn_y),
                        f64::from(x2),
                        f64::from(arrow_y),
                    ),
                ],
                label_center: label
                    .is_some()
                    .then_some((f64::from(x2), f64::from(turn_y + arrow_y) / 2.0)),
                arrow_x: x2,
                arrow_y,
                arrow: "↓",
            }
        }
    }
}

fn distributed_port(origin: i32, length: i32, slot: usize, count: usize) -> f64 {
    let start = f64::from(origin) + 0.5;
    let end = f64::from(origin + length) - 0.5;
    let fraction = (slot + 1) as f64 / (count + 1) as f64;
    start + (end - start) * fraction
}
