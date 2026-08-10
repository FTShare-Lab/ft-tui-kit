use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event;
use serde_json::Value;

use crate::app::App;
use crate::config::RendererConfig;
use crate::data::{DagDataset, load_dag_data};
use crate::interaction::handle_terminal_event;
use crate::render::draw;
use ft_canvas_runtime::{CanvasClient, ClientEvent, IncomingFrame, LaunchConfig};

const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug)]
pub struct LoadResult {
    generation: u64,
    request_id: Option<String>,
    result: Result<DagDataset, String>,
}

pub fn run(
    terminal: &mut ratatui::DefaultTerminal,
    launch: &LaunchConfig,
    client: &CanvasClient,
    client_receiver: Receiver<ClientEvent>,
    load_sender: Sender<LoadResult>,
    load_receiver: Receiver<LoadResult>,
    initial_config: Value,
) -> Result<(), String> {
    let mut app = App::new(initial_config.clone());
    request_load(&mut app, initial_config, None, load_sender.clone());
    let mut ready_generation = None;
    let mut last_state_emit = Instant::now() - STATE_EMIT_INTERVAL;

    while !app.should_exit {
        if app.dirty {
            let mut next_view = app.view;
            terminal
                .draw(|frame| {
                    next_view = draw(frame, &app);
                })
                .map_err(|error| format!("terminal draw failed: {error}"))?;
            app.view = next_view;
            app.offset_x = next_view.offset_x;
            app.offset_y = next_view.offset_y;
            app.dirty = false;
            if app.init_received
                && app.loaded_generation == Some(app.generation)
                && ready_generation != Some(app.generation)
            {
                let title = app.ready_title().or_else(|| launch.title.clone());
                client.send_ready(title.as_deref(), app.generation_request_id.as_deref())?;
                ready_generation = Some(app.generation);
            }
        }

        while let Ok(event) = client_receiver.try_recv() {
            handle_client_event(&mut app, client, event, &load_sender)?;
        }
        while let Ok(result) = load_receiver.try_recv() {
            handle_load_result(&mut app, client, result)?;
        }

        if app.state_dirty && !app.dirty && last_state_emit.elapsed() >= STATE_EMIT_INTERVAL {
            emit_state(&mut app, client)?;
            last_state_emit = Instant::now();
        }

        if event::poll(Duration::from_millis(25))
            .map_err(|error| format!("terminal event poll failed: {error}"))?
        {
            let event =
                event::read().map_err(|error| format!("terminal event read failed: {error}"))?;
            handle_terminal_event(&mut app, client, launch, event)?;
        }
    }

    Ok(())
}

fn request_load(
    app: &mut App,
    value: Value,
    request_id: Option<String>,
    sender: Sender<LoadResult>,
) {
    app.generation += 1;
    let generation = app.generation;
    app.generation_request_id = request_id.clone();
    app.loading = true;
    app.status = "Loading DAG...".to_string();
    app.dirty = true;
    thread::spawn(move || {
        let result = RendererConfig::from_value(value).and_then(load_dag_data);
        let _ = sender.send(LoadResult {
            generation,
            request_id,
            result,
        });
    });
}

fn handle_load_result(
    app: &mut App,
    client: &CanvasClient,
    result: LoadResult,
) -> Result<(), String> {
    if result.generation != app.generation {
        return Ok(());
    }
    let LoadResult {
        generation,
        request_id,
        result,
    } = result;
    match result {
        Ok(dataset) => {
            app.apply_dataset(dataset);
            app.loaded_generation = Some(generation);
        }
        Err(error) => {
            app.loading = false;
            app.status = error.clone();
            app.dirty = true;
            client.send_error(&error, false, request_id.as_deref())?;
        }
    }
    Ok(())
}

fn handle_client_event(
    app: &mut App,
    client: &CanvasClient,
    event: ClientEvent,
    load_sender: &Sender<LoadResult>,
) -> Result<(), String> {
    match event {
        ClientEvent::Frame(frame) => handle_frame(app, client, frame, load_sender),
        ClientEvent::Disconnected { channel } => {
            app.status = format!("Canvas {channel} socket disconnected");
            app.should_exit = true;
            Ok(())
        }
        ClientEvent::Error(error) => {
            app.status = error;
            app.dirty = true;
            Ok(())
        }
    }
}

fn handle_frame(
    app: &mut App,
    client: &CanvasClient,
    frame: IncomingFrame,
    load_sender: &Sender<LoadResult>,
) -> Result<(), String> {
    if frame.channel == "event" {
        if matches!(frame.frame_type.as_str(), "event.nack" | "backpressure") {
            app.status = format!("Canvas host: {}", frame.payload);
            app.dirty = true;
        }
        return Ok(());
    }

    match frame.frame_type.as_str() {
        "init" => {
            app.init_received = true;
            app.dirty = true;
            let config = frame
                .payload
                .get("config")
                .cloned()
                .ok_or_else(|| "init frame is missing payload.config".to_string())?;
            if app.initial_config.as_ref() == Some(&config) {
                app.initial_config = None;
                app.generation_request_id = frame.request_id.clone();
            } else {
                request_load(app, config, frame.request_id.clone(), load_sender.clone());
            }
        }
        "update" => {
            let config = frame
                .payload
                .get("config")
                .cloned()
                .ok_or_else(|| "update frame is missing payload.config".to_string())?;
            request_load(app, config, frame.request_id.clone(), load_sender.clone());
        }
        "request.state" => {
            let request_id = require_request_id(&frame)?;
            let key = frame.payload.get("key").and_then(Value::as_str);
            match app.state_value(key) {
                Ok(state) => client.send_rpc_ok(request_id, state)?,
                Err(error) => client.send_rpc_error(request_id, &error)?,
            }
        }
        "request.selection" => {
            let request_id = require_request_id(&frame)?;
            client.send_rpc_ok(request_id, app.selection_value())?;
        }
        "request.content" => {
            let request_id = require_request_id(&frame)?;
            client.send_rpc_error(request_id, "DAG renderer does not expose editable content")?;
        }
        "ping" => client.send_pong()?,
        "close" => app.should_exit = true,
        "focus" | "registry" => {}
        other => {
            app.status = format!("Ignored unknown control frame: {other}");
            app.dirty = true;
        }
    }
    Ok(())
}

fn require_request_id(frame: &IncomingFrame) -> Result<&str, String> {
    frame
        .request_id
        .as_deref()
        .ok_or_else(|| format!("{} frame is missing requestId", frame.frame_type))
}

fn emit_state(app: &mut App, client: &CanvasClient) -> Result<(), String> {
    if app.dataset.is_none() {
        app.state_dirty = false;
        return Ok(());
    }
    client.send_state("graph", app.state_value(Some("graph"))?)?;
    client.send_state("viewport", app.state_value(Some("viewport"))?)?;
    client.send_state("selection", app.state_value(Some("selection"))?)?;
    client.send_state("activeNode", app.state_value(Some("activeNode"))?)?;
    app.state_dirty = false;
    Ok(())
}
