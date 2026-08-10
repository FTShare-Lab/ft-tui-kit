use ratatui::crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::actions::{export_artifact, send_analysis_action, send_context, send_selection_update};
use crate::app::{App, DragState};
use ft_canvas_runtime::{CanvasClient, LaunchConfig};

pub fn handle_terminal_event(
    app: &mut App,
    client: &CanvasClient,
    launch: &LaunchConfig,
    event: Event,
) -> Result<(), String> {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                client.request_close()?;
                app.should_exit = true;
            }
            KeyCode::Left | KeyCode::Char('h') => app.pan(-4, 0),
            KeyCode::Right | KeyCode::Char('l') => app.pan(4, 0),
            KeyCode::Up | KeyCode::Char('k') => app.pan(0, -2),
            KeyCode::Down | KeyCode::Char('j') => app.pan(0, 2),
            KeyCode::Home => app.reset_view(),
            KeyCode::Tab => app.cycle_active(false),
            KeyCode::BackTab => app.cycle_active(true),
            KeyCode::Char(' ') => {
                let active = app.active_node;
                if app.toggle_node(active) {
                    send_selection_update(app, client)?;
                }
            }
            KeyCode::Char('c') => {
                app.clear_selection();
                send_selection_update(app, client)?;
            }
            KeyCode::Char('a') => send_context(app, client)?,
            KeyCode::Enter => send_analysis_action(app, client)?,
            KeyCode::Char('e') => export_artifact(app, client, launch)?,
            _ => {}
        },
        Event::Mouse(mouse) => handle_mouse(app, client, mouse)?,
        Event::Resize(_, _) => {
            app.dirty = true;
            app.state_dirty = true;
        }
        _ => {}
    }
    Ok(())
}

fn handle_mouse(app: &mut App, client: &CanvasClient, mouse: MouseEvent) -> Result<(), String> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left)
            if contains(app.view.area, mouse.column, mouse.row) =>
        {
            app.drag = Some(DragState {
                last_column: mouse.column,
                last_row: mouse.row,
                pressed_node: hit_node(app, mouse.column, mouse.row),
                moved: false,
            });
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let Some(mut drag) = app.drag else {
                return Ok(());
            };
            let dx = i32::from(drag.last_column) - i32::from(mouse.column);
            let dy = i32::from(drag.last_row) - i32::from(mouse.row);
            if dx != 0 || dy != 0 {
                drag.moved = true;
                app.pan(dx, dy);
            }
            drag.last_column = mouse.column;
            drag.last_row = mouse.row;
            app.drag = Some(drag);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let Some(drag) = app.drag.take() else {
                return Ok(());
            };
            if !drag.moved
                && drag.pressed_node == hit_node(app, mouse.column, mouse.row)
                && let Some(index) = drag.pressed_node
                && app.toggle_node(index)
            {
                send_selection_update(app, client)?;
            }
        }
        MouseEventKind::ScrollDown => {
            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                app.pan(6, 0);
            } else {
                app.pan(0, 3);
            }
        }
        MouseEventKind::ScrollUp => {
            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                app.pan(-6, 0);
            } else {
                app.pan(0, -3);
            }
        }
        MouseEventKind::ScrollLeft => app.pan(-6, 0),
        MouseEventKind::ScrollRight => app.pan(6, 0),
        _ => {}
    }
    Ok(())
}

fn hit_node(app: &App, column: u16, row: u16) -> Option<usize> {
    if !contains(app.view.area, column, row) {
        return None;
    }
    let world_x = app.view.offset_x + i32::from(column - app.view.area.x);
    let world_y = app.view.offset_y + i32::from(row - app.view.area.y);
    app.graph
        .nodes
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, rect)| rect.contains(world_x, world_y).then_some(index))
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}
