use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::app::{App, ViewInfo};
use crate::data::{DagDataset, EdgeSpec};
use crate::layout::{WorldRect, text_width, truncate_text, wrap_text};

const BACKGROUND: Color = Color::Rgb(9, 18, 26);
const NODE_BACKGROUND: Color = Color::Rgb(17, 36, 47);
const SELECTED_BACKGROUND: Color = Color::Rgb(50, 42, 19);
const MUTED: Color = Color::Rgb(121, 145, 153);
const HIGHLIGHT: Color = Color::Rgb(255, 196, 72);

pub fn draw(frame: &mut Frame, app: &App) -> ViewInfo {
    let [header_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::new().fg(MUTED).bg(BACKGROUND)),
        status_area,
    );

    let Some(dataset) = app.dataset.as_ref() else {
        frame.render_widget(
            Paragraph::new(app.status.as_str())
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" DAG Canvas "),
                ),
            Rect::new(
                header_area.x,
                header_area.y,
                header_area.width,
                content_area.bottom().saturating_sub(header_area.y),
            ),
        );
        return ViewInfo::empty();
    };

    render_header(frame, dataset, header_area);
    let viewport_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(54, 92, 104)))
        .style(Style::new().bg(BACKGROUND))
        .title(" DAG | drag to pan ");
    let graph_area = viewport_block.inner(content_area);
    frame.render_widget(viewport_block, content_area);

    if graph_area.width == 0 || graph_area.height == 0 {
        return ViewInfo {
            area: graph_area,
            offset_x: 0,
            offset_y: 0,
        };
    }
    let (offset_x, offset_y) = clamped_offsets(app, graph_area);
    let view = ViewInfo {
        area: graph_area,
        offset_x,
        offset_y,
    };
    render_edges(frame, app, dataset, view);
    render_edge_decorations(frame, app, dataset, view);
    render_nodes(frame, app, dataset, view);
    view
}

fn render_header(frame: &mut Frame, dataset: &DagDataset, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            dataset.document.title.clone(),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  {} nodes / {} edges",
                dataset.document.nodes.len(),
                dataset.document.edges.len()
            ),
            Style::new().fg(HIGHLIGHT),
        ),
    ]);
    let subtitle = format!(
        "{} | {} | source: {}",
        dataset.document.subtitle.as_deref().unwrap_or("DAG"),
        dataset.document.layout.direction.as_str(),
        dataset.source
    );
    frame.render_widget(
        Paragraph::new(vec![title, Line::from(subtitle)]).style(Style::new().bg(BACKGROUND)),
        area,
    );
}

fn render_edges(frame: &mut Frame, app: &App, dataset: &DagDataset, view: ViewInfo) {
    let x_bounds = [
        f64::from(view.offset_x),
        f64::from(view.offset_x + i32::from(view.area.width)),
    ];
    let y_bounds = [
        -f64::from(view.offset_y + i32::from(view.area.height)),
        -f64::from(view.offset_y),
    ];
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .background_color(BACKGROUND)
        .x_bounds(x_bounds)
        .y_bounds(y_bounds)
        .paint(|context| {
            for (edge_index, edge) in dataset.document.edges.iter().enumerate() {
                let from_index = dataset.node_index[&edge.from];
                let to_index = dataset.node_index[&edge.to];
                let color = edge_color(app, edge, from_index, to_index);
                let geometry = &app.graph.edges[edge_index];
                for &(x1, y1, x2, y2) in &geometry.segments {
                    context.draw(&CanvasLine::new(x1, -y1, x2, -y2, color));
                }
            }
        });
    frame.render_widget(canvas, view.area);
}

fn render_edge_decorations(frame: &mut Frame, app: &App, dataset: &DagDataset, view: ViewInfo) {
    for (edge_index, edge) in dataset.document.edges.iter().enumerate() {
        let from_index = dataset.node_index[&edge.from];
        let to_index = dataset.node_index[&edge.to];
        let color = edge_color(app, edge, from_index, to_index);
        let geometry = &app.graph.edges[edge_index];

        if let (Some(label), Some((center_x, center_y))) =
            (edge.label.as_deref(), geometry.label_center)
        {
            let width = text_width(label).max(1).min(usize::from(u16::MAX));
            let world_left = (center_x - width as f64 / 2.0).round() as i32;
            let world_top = center_y.round() as i32;
            if let Some(area) = decoration_rect(world_left, world_top, width as u16, view) {
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(label).style(Style::new().fg(color).bg(BACKGROUND)),
                    area,
                );
            }
        }

        if let Some(area) = decoration_rect(geometry.arrow_x, geometry.arrow_y, 1, view) {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(geometry.arrow).style(
                    Style::new()
                        .fg(color)
                        .bg(BACKGROUND)
                        .add_modifier(Modifier::BOLD),
                ),
                area,
            );
        }
    }
}

fn render_nodes(frame: &mut Frame, app: &App, dataset: &DagDataset, view: ViewInfo) {
    for (index, node) in dataset.document.nodes.iter().enumerate() {
        let Some(area) = screen_rect(app.graph.nodes[index], view) else {
            continue;
        };
        if area.width < 3 || area.height < 2 {
            continue;
        }
        let selected = app.selected.contains(&index);
        let active = app.active_node == index;
        let border_color = if selected {
            HIGHLIGHT
        } else {
            parse_color(&node.color)
        };
        let background = if selected {
            SELECTED_BACKGROUND
        } else {
            NODE_BACKGROUND
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border_color).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }))
            .style(Style::new().bg(background))
            .title(format!(" L{} ", dataset.ranks[index]));
        let text_width = usize::from(area.width.saturating_sub(2)).max(1);
        let max_lines = usize::from(area.height.saturating_sub(2));
        let mut lines = Vec::new();
        for text in wrap_text(&node.label, text_width) {
            if lines.len() >= max_lines {
                break;
            }
            lines.push(Line::from(Span::styled(
                text,
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
        }
        if lines.len() < max_lines {
            lines.push(Line::from(Span::styled(
                truncate_text(&node.id, text_width),
                Style::new().fg(if active { HIGHLIGHT } else { MUTED }),
            )));
        }
        if let Some(description) = node.description.as_deref() {
            for text in wrap_text(description, text_width) {
                if lines.len() >= max_lines {
                    break;
                }
                lines.push(Line::from(Span::styled(text, Style::new().fg(MUTED))));
            }
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .block(block),
            area,
        );
    }
}

fn edge_color(app: &App, edge: &EdgeSpec, from: usize, to: usize) -> Color {
    if app.selected.contains(&from) || app.selected.contains(&to) {
        HIGHLIGHT
    } else if let Some(configured) = edge.color.as_deref() {
        parse_color(configured)
    } else {
        generated_edge_color(&edge.to)
    }
}

fn generated_edge_color(target_id: &str) -> Color {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in target_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let hue = (hash % 360) as f64;
    let saturation = 0.68 + ((hash >> 9) % 18) as f64 / 100.0;
    let value = 0.88 + ((hash >> 17) % 9) as f64 / 100.0;
    let chroma = value * saturation;
    let sector = hue / 60.0;
    let secondary = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let minimum = value - chroma;
    Color::Rgb(
        rgb_channel(red + minimum),
        rgb_channel(green + minimum),
        rgb_channel(blue + minimum),
    )
}

fn rgb_channel(value: f64) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

fn clamped_offsets(app: &App, area: Rect) -> (i32, i32) {
    let max_x = (app.graph.width - i32::from(area.width)).max(0);
    let max_y = (app.graph.height - i32::from(area.height)).max(0);
    (app.offset_x.clamp(0, max_x), app.offset_y.clamp(0, max_y))
}

fn screen_rect(world: WorldRect, view: ViewInfo) -> Option<Rect> {
    let left = i32::from(view.area.x) + world.x - view.offset_x;
    let top = i32::from(view.area.y) + world.y - view.offset_y;
    let right = left + world.width;
    let bottom = top + world.height;
    let clipped_left = left.max(i32::from(view.area.x));
    let clipped_top = top.max(i32::from(view.area.y));
    let clipped_right = right.min(i32::from(view.area.right()));
    let clipped_bottom = bottom.min(i32::from(view.area.bottom()));
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        return None;
    }
    Some(Rect::new(
        clipped_left as u16,
        clipped_top as u16,
        (clipped_right - clipped_left) as u16,
        (clipped_bottom - clipped_top) as u16,
    ))
}

fn decoration_rect(world_x: i32, world_y: i32, width: u16, view: ViewInfo) -> Option<Rect> {
    let screen_x = i32::from(view.area.x) + world_x - view.offset_x;
    let screen_y = i32::from(view.area.y) + world_y - view.offset_y;
    if screen_x < i32::from(view.area.x)
        || screen_y < i32::from(view.area.y)
        || screen_x + i32::from(width) > i32::from(view.area.right())
        || screen_y >= i32::from(view.area.bottom())
    {
        return None;
    }
    Some(Rect::new(screen_x as u16, screen_y as u16, width, 1))
}

fn parse_color(value: &str) -> Color {
    match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "white" => Color::White,
        text if text.starts_with('#') && text.len() == 7 => {
            let red = u8::from_str_radix(&text[1..3], 16).unwrap_or(255);
            let green = u8::from_str_radix(&text[3..5], 16).unwrap_or(255);
            let blue = u8::from_str_radix(&text[5..7], 16).unwrap_or(255);
            Color::Rgb(red, green, blue)
        }
        _ => Color::White,
    }
}
