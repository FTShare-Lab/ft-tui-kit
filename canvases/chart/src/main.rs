mod config;
mod data;

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use config::RendererConfig;
use data::{ChartDataset, ChartView, ShowValues, load_chart_data};
use ft_canvas_runtime::{CanvasClient, ClientEvent, IncomingFrame, LaunchConfig};
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, Borders, Chart, Dataset, GraphType, Paragraph,
};
use serde_json::{Value, json};

const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(120);
const ZOOM_OFFSETS: [i16; 5] = [-2, -1, 0, 2, 5];
const DEFAULT_ZOOM: usize = 2;
const MAX_SELECTED_CELLS: usize = 512;
const BACKGROUND: Color = Color::Rgb(13, 17, 23);
const AXIS_COLOR: Color = Color::Rgb(120, 123, 134);
const HIGHLIGHT: Color = Color::Rgb(255, 214, 102);

fn main() {
    if let Err(error) = start() {
        eprintln!("chart: {error}");
        std::process::exit(1);
    }
}

fn start() -> Result<(), String> {
    let launch_path = parse_launch_file()?;
    let launch = LaunchConfig::read(&launch_path)?;
    let initial_config = read_json_file(&launch.config_path)?;
    let (client_sender, client_receiver) = mpsc::channel();
    let client = CanvasClient::connect(launch.clone(), client_sender)?;
    let (load_sender, load_receiver) = mpsc::channel();

    let mut terminal = ratatui::init();
    if let Err(error) = execute!(io::stdout(), EnableMouseCapture) {
        ratatui::restore();
        client.close();
        return Err(format!("cannot enable terminal mouse capture: {error}"));
    }

    let result = run(
        &mut terminal,
        &launch,
        &client,
        client_receiver,
        load_sender,
        load_receiver,
        initial_config,
    );
    let mouse_result = execute!(io::stdout(), DisableMouseCapture)
        .map_err(|error| format!("cannot disable terminal mouse capture: {error}"));
    ratatui::restore();
    client.close();

    result.and(mouse_result)
}

fn parse_launch_file() -> Result<PathBuf, String> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--launch-file" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--launch-file requires a path".to_string());
        }
    }
    Err("usage: chart --launch-file <launch.json>".to_string())
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open config file {}: {error}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|error| format!("invalid config file {}: {error}", path.display()))
}

#[derive(Debug)]
struct LoadResult {
    generation: u64,
    request_id: Option<String>,
    result: Result<ChartDataset, LoadFailure>,
}

#[derive(Debug)]
enum LoadFailure {
    Config(String),
    Data(String),
}

#[derive(Debug, Clone, Copy)]
struct ViewInfo {
    bar_plot: Rect,
    line_plot: Rect,
    first: usize,
    last: usize,
    bar_width: u16,
    bar_gap: u16,
    group_gap: u16,
    series_count: usize,
    active_series: usize,
}

impl ViewInfo {
    fn empty() -> Self {
        Self {
            bar_plot: Rect::new(0, 0, 0, 0),
            line_plot: Rect::new(0, 0, 0, 0),
            first: 0,
            last: 0,
            bar_width: 1,
            bar_gap: 1,
            group_gap: 1,
            series_count: 0,
            active_series: 0,
        }
    }
}

struct App {
    right: usize,
    zoom: usize,
    active_series: usize,
    selected: BTreeSet<(usize, usize)>,
    dataset: Option<ChartDataset>,
    view: ViewInfo,
    status: String,
    loading: bool,
    generation: u64,
    loaded_generation: Option<u64>,
    generation_request_id: Option<String>,
    initial_config: Option<Value>,
    init_received: bool,
    dirty: bool,
    state_dirty: bool,
    should_exit: bool,
}

impl App {
    fn new(initial_config: Value) -> Self {
        Self {
            right: 0,
            zoom: DEFAULT_ZOOM,
            active_series: 0,
            selected: BTreeSet::new(),
            dataset: None,
            view: ViewInfo::empty(),
            status: "Loading chart data...".to_string(),
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

    fn total(&self) -> usize {
        self.dataset.as_ref().map_or(0, ChartDataset::row_count)
    }

    fn geometry(&self) -> (u16, u16, u16) {
        let Some(dataset) = self.dataset.as_ref() else {
            return (1, 1, 1);
        };
        let display = &dataset.document.display;
        let width = (display.bar_width as i16 + ZOOM_OFFSETS[self.zoom]).clamp(1, 8) as u16;
        (width, display.bar_gap, display.group_gap)
    }

    fn clamp(&mut self) {
        let total = self.total();
        self.right = self.right.min(total);
        if total > 0 && self.right == 0 {
            self.right = 1;
        }
        let series_count = self.dataset.as_ref().map_or(0, ChartDataset::series_count);
        self.active_series = self.active_series.min(series_count.saturating_sub(1));
        self.selected
            .retain(|(row, series)| *row < total && *series < series_count);
    }

    fn pan_left(&mut self) {
        self.right = self.right.saturating_sub(1).max(1);
        self.changed_view();
    }

    fn pan_right(&mut self) {
        self.right = (self.right + 1).min(self.total());
        self.changed_view();
    }

    fn zoom_in(&mut self) {
        let next = (self.zoom + 1).min(ZOOM_OFFSETS.len() - 1);
        if next != self.zoom {
            self.zoom = next;
            self.changed_view();
        }
    }

    fn zoom_out(&mut self) {
        let next = self.zoom.saturating_sub(1);
        if next != self.zoom {
            self.zoom = next;
            self.changed_view();
        }
    }

    fn previous_series(&mut self) {
        self.active_series = self.active_series.saturating_sub(1);
        self.changed_view();
    }

    fn next_series(&mut self) {
        let count = self.dataset.as_ref().map_or(0, ChartDataset::series_count);
        self.active_series = (self.active_series + 1).min(count.saturating_sub(1));
        self.changed_view();
    }

    fn toggle_selection(&mut self, row: usize, series: usize) -> bool {
        self.active_series = series;
        self.state_dirty = true;
        if self.selected.remove(&(row, series)) {
            self.dirty = true;
            self.state_dirty = true;
            return true;
        }
        if self.selected.len() >= MAX_SELECTED_CELLS {
            self.status = format!("Selection is limited to {MAX_SELECTED_CELLS} chart cells");
            self.dirty = true;
            return false;
        }
        self.selected.insert((row, series));
        self.dirty = true;
        self.state_dirty = true;
        true
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn changed_view(&mut self) {
        self.clamp();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn apply_dataset(&mut self, dataset: ChartDataset) {
        let same_shape = self.dataset.as_ref().is_some_and(|current| {
            current.config.data_file == dataset.config.data_file
                && current.categories == dataset.categories
                && current.document.series.len() == dataset.document.series.len()
                && current
                    .document
                    .series
                    .iter()
                    .zip(&dataset.document.series)
                    .all(|(left, right)| left.field == right.field)
        });
        let was_at_right = self.total() == 0 || self.right == self.total();

        if !same_shape {
            self.selected.clear();
            self.zoom = DEFAULT_ZOOM;
            self.active_series = 0;
        }
        self.dataset = Some(dataset);
        self.view = ViewInfo::empty();
        self.right = if was_at_right {
            self.total()
        } else {
            self.right.min(self.total())
        };
        self.loading = false;
        self.status = self
            .dataset
            .as_ref()
            .map(|dataset| {
                format!(
                    "{} rows | {} series | a attach | Enter analyze | e export | c clear | q close",
                    dataset.row_count(),
                    dataset.series_count()
                )
            })
            .unwrap_or_default();
        self.clamp();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn selection_value(&self) -> Value {
        let Some(dataset) = self.dataset.as_ref() else {
            return Value::Null;
        };
        let cells: Vec<Value> = self
            .selected
            .iter()
            .map(|(row, series)| cell_value(dataset, *row, *series))
            .collect();
        json!({
            "dataFile": dataset.config.data_file,
            "table": dataset.document.table.name,
            "count": cells.len(),
            "cells": cells
        })
    }

    fn state_value(&self, key: Option<&str>) -> Result<Value, String> {
        let Some(dataset) = self.dataset.as_ref() else {
            return Ok(json!({ "status": self.status, "loading": self.loading }));
        };
        let visible_range = visible_range_value(dataset, &self.view);
        let selection = self.selection_value();
        let zoom = json!({
            "level": self.zoom,
            "levels": ZOOM_OFFSETS.len(),
            "barWidth": self.view.bar_width,
            "visibleCategories": self.view.last.saturating_sub(self.view.first),
            "view": dataset.document.display.view.as_str()
        });
        let active_series = dataset
            .document
            .series
            .get(self.active_series)
            .map(|series| {
                json!({
                    "index": self.active_series,
                    "field": series.field,
                    "label": series.label
                })
            })
            .unwrap_or(Value::Null);
        let all = json!({
            "dataFile": dataset.config.data_file,
            "table": dataset.document.table.name,
            "title": dataset.document.title,
            "view": dataset.document.display.view.as_str(),
            "visibleRange": visible_range.clone(),
            "selection": selection.clone(),
            "zoom": zoom.clone(),
            "activeSeries": active_series.clone(),
            "source": dataset.source,
            "loading": self.loading
        });
        match key {
            None => Ok(all),
            Some("dataFile") => Ok(json!(dataset.config.data_file)),
            Some("table") => Ok(json!(dataset.document.table.name)),
            Some("view") => Ok(json!(dataset.document.display.view.as_str())),
            Some("visibleRange") => Ok(visible_range),
            Some("selection") => Ok(selection),
            Some("zoom") => Ok(zoom),
            Some("activeSeries") => Ok(active_series),
            Some("source") => Ok(json!(dataset.source)),
            Some(other) => Err(format!("unknown state key: {other}")),
        }
    }

    fn ready_title(&self) -> Option<String> {
        self.dataset
            .as_ref()
            .map(|dataset| dataset.document.title.clone())
    }
}

fn run(
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
    app.status = "Loading chart data...".to_string();
    app.dirty = true;
    thread::spawn(move || {
        let result = match RendererConfig::from_value(value) {
            Ok(config) => load_chart_data(config).map_err(LoadFailure::Data),
            Err(error) => Err(LoadFailure::Config(error)),
        };
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
        Err(LoadFailure::Config(error)) => {
            app.loading = false;
            app.status = error.clone();
            app.dirty = true;
            client.send_config_error(&error, request_id.as_deref())?;
        }
        Err(LoadFailure::Data(message)) => {
            app.loading = false;
            app.status = message.clone();
            app.dirty = true;
            client.send_data_error(&message, request_id.as_deref())?;
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
            client.send_rpc_error(
                request_id,
                "chart renderer does not expose editable content",
            )?;
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

fn handle_terminal_event(
    app: &mut App,
    client: &CanvasClient,
    launch: &LaunchConfig,
    event: Event,
) -> Result<(), String> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                client.request_close()?;
                app.should_exit = true;
            }
            KeyCode::Left => app.pan_left(),
            KeyCode::Right => app.pan_right(),
            KeyCode::Up => app.previous_series(),
            KeyCode::Down => app.next_series(),
            KeyCode::Char('+') | KeyCode::Char('=') => app.zoom_in(),
            KeyCode::Char('-') => app.zoom_out(),
            KeyCode::Char('c') => {
                app.clear_selection();
                send_selection(app, client)?;
            }
            KeyCode::Char('a') => send_context(app, client)?,
            KeyCode::Char('e') => export_artifact(app, client, launch)?,
            KeyCode::Enter => send_analysis_action(app, client)?,
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
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some((row, series)) = hit_selection(&app.view, mouse.column, mouse.row) {
                if app.toggle_selection(row, series) {
                    send_selection(app, client)?;
                }
            }
        }
        MouseEventKind::ScrollUp => app.zoom_in(),
        MouseEventKind::ScrollDown => app.zoom_out(),
        _ => {}
    }
    Ok(())
}

fn send_selection(app: &App, client: &CanvasClient) -> Result<(), String> {
    let count = app.selected.len();
    let text = if count == 0 {
        "The chart selection was cleared.".to_string()
    } else {
        format!("The user selected {count} cell(s) on the grouped bar chart.")
    };
    client.send_selection("selected chart cells", text, app.selection_value())
}

fn send_context(app: &App, client: &CanvasClient) -> Result<(), String> {
    let (text, data) = summarized_context(app)?;
    client.send_context("statistical chart context", text, data)
}

fn send_analysis_action(app: &App, client: &CanvasClient) -> Result<(), String> {
    let (summary, data) = summarized_context(app)?;
    let prompt = format!(
        "Analyze the selected statistical chart cells, or the visible range when nothing is selected. Compare series, identify changes, outliers, ratios, and financially relevant implications. Chart summary: {summary}"
    );
    client.send_action("analyze chart data", prompt, data)
}

fn summarized_context(app: &App) -> Result<(String, Value), String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "chart data is not loaded yet".to_string())?;
    let cells = context_cell_keys(app, dataset);
    if cells.is_empty() {
        return Err("no chart cells are available".to_string());
    }

    let mut rows = BTreeSet::new();
    let mut aggregates: BTreeMap<usize, (usize, f64, f64, f64)> = BTreeMap::new();
    for (row, series) in &cells {
        let Some(value) = dataset.value(*row, *series) else {
            continue;
        };
        rows.insert(*row);
        let aggregate = aggregates
            .entry(*series)
            .or_insert((0, 0.0, f64::MAX, f64::MIN));
        aggregate.0 += 1;
        aggregate.1 += value;
        aggregate.2 = aggregate.2.min(value);
        aggregate.3 = aggregate.3.max(value);
    }

    let series_summary: Vec<Value> = aggregates
        .iter()
        .filter_map(|(series_index, (count, sum, minimum, maximum))| {
            let series = dataset.document.series.get(*series_index)?;
            Some(json!({
                "index": series_index,
                "field": series.field,
                "label": series.label,
                "count": count,
                "sum": sum,
                "mean": sum / *count as f64,
                "min": minimum,
                "max": maximum
            }))
        })
        .collect();
    let cell_values: Vec<Value> = cells
        .iter()
        .map(|(row, series)| cell_value(dataset, *row, *series))
        .collect();
    let summary = format!(
        "{} / {}, {} row(s), {} cell(s), {} series",
        dataset.document.title,
        dataset.document.table.name,
        rows.len(),
        cells.len(),
        aggregates.len()
    );

    Ok((
        summary.clone(),
        json!({
            "summary": summary,
            "dataFile": dataset.config.data_file,
            "table": dataset.document.table.name,
            "title": dataset.document.title,
            "selectionApplied": !app.selected.is_empty(),
            "visibleRange": visible_range_value(dataset, &app.view),
            "rowIndices": rows,
            "cells": cell_values,
            "seriesSummary": series_summary,
            "metadata": dataset.document.metadata
        }),
    ))
}

fn export_artifact(app: &App, client: &CanvasClient, launch: &LaunchConfig) -> Result<(), String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "chart data is not loaded yet".to_string())?;
    let cells = context_cell_keys(app, dataset);
    if cells.is_empty() {
        return Err("no chart data is available to export".to_string());
    }
    let row_indices: BTreeSet<usize> = cells.iter().map(|(row, _)| *row).collect();
    let rows: Vec<Value> = row_indices
        .iter()
        .filter_map(|index| {
            dataset
                .document
                .table
                .rows
                .get(*index)
                .map(|row| json!({ "index": index, "data": row }))
        })
        .collect();
    let selected_cells: Vec<Value> = cells
        .iter()
        .map(|(row, series)| cell_value(dataset, *row, *series))
        .collect();
    let path = launch.runtime_dir.join(format!(
        "{}-chart-{}.json",
        safe_filename(&dataset.document.table.name),
        now_millis()
    ));
    let artifact = json!({
        "schemaVersion": 1,
        "sourceDataFile": dataset.config.data_file,
        "title": dataset.document.title,
        "subtitle": dataset.document.subtitle,
        "table": dataset.document.table.name,
        "selectionApplied": !app.selected.is_empty(),
        "series": dataset.document.series,
        "rows": rows,
        "cells": selected_cells,
        "metadata": dataset.document.metadata
    });
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|error| format!("cannot encode chart artifact: {error}"))?;
    std::fs::write(&path, bytes)
        .map_err(|error| format!("cannot write artifact {}: {error}", path.display()))?;
    client.send_artifact(
        "chart data",
        &path,
        format!("Exported {} row(s) from the chart.", row_indices.len()),
    )
}

fn emit_state(app: &mut App, client: &CanvasClient) -> Result<(), String> {
    if app.dataset.is_none() {
        return Ok(());
    }
    client.send_state("visibleRange", app.state_value(Some("visibleRange"))?)?;
    client.send_state("selection", app.state_value(Some("selection"))?)?;
    client.send_state("zoom", app.state_value(Some("zoom"))?)?;
    client.send_state("activeSeries", app.state_value(Some("activeSeries"))?)?;
    app.state_dirty = false;
    Ok(())
}

fn draw(frame: &mut Frame, app: &App) -> ViewInfo {
    let [header_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let status_style = Style::new()
        .fg(if app.loading {
            Color::Yellow
        } else {
            Color::Gray
        })
        .bg(BACKGROUND);
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(status_style),
        status_area,
    );

    let Some(dataset) = app.dataset.as_ref() else {
        frame.render_widget(
            Paragraph::new(app.status.as_str())
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Chart Canvas "),
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

    render_header(frame, dataset, app.active_series, header_area);
    let (bar_width, bar_gap, group_gap) = app.geometry();
    let (bars_area, line_area) = match dataset.document.display.view {
        ChartView::Bar => (Some(content_area), None),
        ChartView::Line => (None, Some(content_area)),
        ChartView::Both => {
            let [bars, line] = Layout::vertical([Constraint::Percentage(62), Constraint::Min(8)])
                .areas(content_area);
            (Some(bars), Some(line))
        }
    };
    let capacity = if let Some(area) = bars_area {
        bar_capacity(
            inset(area, 1).width,
            dataset.series_count(),
            bar_width,
            bar_gap,
            group_gap,
        )
    } else {
        line_capacity(inset(line_area.unwrap_or(content_area), 1).width, bar_width)
    }
    .min(dataset.row_count());
    let last = app.right.min(dataset.row_count());
    let first = if capacity == 0 {
        last
    } else {
        last.saturating_sub(capacity)
    };
    let maximum = dataset.axis_max(first..last);
    let mut bar_plot = Rect::new(0, 0, 0, 0);
    if let Some(area) = bars_area {
        bar_plot = inset(area, 1);
        render_bars(
            frame,
            dataset,
            &app.selected,
            first,
            last,
            maximum,
            bar_width,
            bar_gap,
            group_gap,
            area,
        );
    }
    let mut line_plot = Rect::new(0, 0, 0, 0);
    if let Some(area) = line_area {
        line_plot = line_hit_area(dataset, maximum, area);
        render_overview(
            frame,
            dataset,
            &app.selected,
            app.active_series,
            first,
            last,
            maximum,
            area,
        );
    }

    ViewInfo {
        bar_plot,
        line_plot,
        first,
        last,
        bar_width,
        bar_gap,
        group_gap,
        series_count: dataset.series_count(),
        active_series: app.active_series,
    }
}

fn render_header(frame: &mut Frame, dataset: &ChartDataset, active: usize, area: Rect) {
    let mut title = vec![Span::styled(
        dataset.document.title.clone(),
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
    )];
    if dataset.document.display.show_legend {
        title.push(Span::raw("  "));
        for (index, series) in dataset.document.series.iter().enumerate() {
            let modifier = if index == active {
                Modifier::BOLD | Modifier::UNDERLINED
            } else {
                Modifier::empty()
            };
            title.push(Span::styled(
                format!("■ {}  ", series.label),
                Style::new()
                    .fg(parse_color(&series.color))
                    .add_modifier(modifier),
            ));
        }
    }
    let subtitle = format!(
        "{} | view: {} | table: {} | source: {}",
        dataset.document.subtitle.as_deref().unwrap_or(""),
        dataset.document.display.view.as_str(),
        dataset.document.table.name,
        dataset.source
    );
    frame.render_widget(
        Paragraph::new(vec![Line::from(title), Line::from(subtitle)])
            .style(Style::new().bg(BACKGROUND)),
        area,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_bars(
    frame: &mut Frame,
    dataset: &ChartDataset,
    selected: &BTreeSet<(usize, usize)>,
    first: usize,
    last: usize,
    maximum: f64,
    bar_width: u16,
    bar_gap: u16,
    group_gap: u16,
    area: Rect,
) {
    let max_scaled = (maximum * dataset.scale() as f64)
        .round()
        .clamp(1.0, u64::MAX as f64) as u64;
    let title = format!(
        " Grouped bars | {} | click select | wheel zoom ",
        dataset
            .document
            .axes
            .x
            .label
            .as_deref()
            .unwrap_or("category")
    );
    let mut chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::new().bg(BACKGROUND))
        .bar_width(bar_width)
        .bar_gap(bar_gap)
        .group_gap(group_gap)
        .max(max_scaled);

    for row_index in first..last {
        let bars: Vec<Bar<'static>> = dataset
            .document
            .series
            .iter()
            .enumerate()
            .map(|(series_index, series)| {
                let is_selected = selected.contains(&(row_index, series_index));
                let base_color = parse_color(&series.color);
                let style = if is_selected {
                    Style::new()
                        .fg(Color::Black)
                        .bg(HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(base_color)
                };
                let value = dataset.value(row_index, series_index).unwrap_or_default();
                let text_value = match dataset.document.display.show_values {
                    ShowValues::Always => dataset.format_value(value),
                    ShowValues::Selected if is_selected => dataset.format_value(value),
                    ShowValues::Never | ShowValues::Selected => String::new(),
                };
                Bar::default()
                    .label(short_label(&series.label, bar_width as usize))
                    .value(dataset.scaled_value(row_index, series_index))
                    .text_value(text_value)
                    .style(style)
                    .value_style(style)
            })
            .collect();
        chart = chart.data(BarGroup::new(bars).label(short_label(
            &dataset.categories[row_index],
            group_label_width(dataset, bar_width, bar_gap),
        )));
    }
    frame.render_widget(chart, area);
}

#[allow(clippy::too_many_arguments)]
fn render_overview(
    frame: &mut Frame,
    dataset: &ChartDataset,
    selected: &BTreeSet<(usize, usize)>,
    active_series: usize,
    first: usize,
    last: usize,
    maximum: f64,
    area: Rect,
) {
    let graph_type = if last.saturating_sub(first) <= 1 {
        GraphType::Scatter
    } else {
        GraphType::Line
    };
    let points: Vec<Vec<(f64, f64)>> = (0..dataset.series_count())
        .map(|series| {
            (first..last)
                .filter_map(|row| {
                    dataset
                        .value(row, series)
                        .map(|value| ((row - first) as f64, value))
                })
                .collect()
        })
        .collect();
    let selected_points: Vec<(f64, f64)> = selected
        .iter()
        .filter(|(row, _)| *row >= first && *row < last)
        .filter_map(|(row, series)| {
            dataset
                .value(*row, *series)
                .map(|value| ((*row - first) as f64, value))
        })
        .collect();
    let mut datasets: Vec<Dataset<'_>> = dataset
        .document
        .series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            let marker = if index == active_series {
                Marker::Braille
            } else {
                Marker::Dot
            };
            Dataset::default()
                .name(series.label.clone())
                .marker(marker)
                .graph_type(graph_type)
                .style(Style::new().fg(parse_color(&series.color)))
                .data(&points[index])
        })
        .collect();
    if !selected_points.is_empty() {
        datasets.push(
            Dataset::default()
                .name("selected")
                .marker(Marker::Block)
                .graph_type(GraphType::Scatter)
                .style(Style::new().fg(HIGHLIGHT))
                .data(&selected_points),
        );
    }

    let visible = last.saturating_sub(first);
    let x_max = visible.saturating_sub(1).max(1) as f64;
    let x_labels = axis_category_labels(dataset, first, last);
    let y_labels = vec![
        dataset.format_value(0.0),
        dataset.format_value(maximum / 2.0),
        dataset.format_value(maximum),
    ];
    let x_axis = Axis::default()
        .title(
            dataset
                .document
                .axes
                .x
                .label
                .clone()
                .unwrap_or_else(|| "Category".to_string()),
        )
        .style(Style::new().fg(AXIS_COLOR))
        .bounds([0.0, x_max])
        .labels(x_labels);
    let y_axis = Axis::default()
        .title(
            dataset
                .document
                .axes
                .y
                .label
                .clone()
                .unwrap_or_else(|| "Value".to_string()),
        )
        .style(Style::new().fg(AXIS_COLOR))
        .bounds([0.0, maximum])
        .labels(y_labels);
    let active_label = dataset
        .document
        .series
        .get(active_series)
        .map(|series| series.label.as_str())
        .unwrap_or("none");
    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title(format!(
            " Line chart | active: {active_label} | click category | ↑/↓ series "
        )))
        .style(Style::new().bg(BACKGROUND))
        .x_axis(x_axis)
        .y_axis(y_axis)
        .legend_position(None);
    frame.render_widget(chart, area);
}

fn hit_selection(view: &ViewInfo, column: u16, row: u16) -> Option<(usize, usize)> {
    hit_bar(view, column, row).or_else(|| hit_line(view, column, row))
}

fn hit_bar(view: &ViewInfo, column: u16, row: u16) -> Option<(usize, usize)> {
    if !contains(view.bar_plot, column, row) || view.series_count == 0 {
        return None;
    }
    let bar_stride = usize::from(view.bar_width + view.bar_gap);
    let group_stride = view
        .series_count
        .saturating_mul(bar_stride)
        .saturating_add(usize::from(view.group_gap));
    if group_stride == 0 {
        return None;
    }
    let local = usize::from(column - view.bar_plot.x);
    let visible_row = local / group_stride;
    let within_group = local % group_stride;
    let series = within_group / bar_stride;
    let within_bar = within_group % bar_stride;
    let absolute_row = view.first + visible_row;
    if absolute_row >= view.last
        || series >= view.series_count
        || within_bar >= view.bar_width as usize
    {
        return None;
    }
    Some((absolute_row, series))
}

fn hit_line(view: &ViewInfo, column: u16, row: u16) -> Option<(usize, usize)> {
    if !contains(view.line_plot, column, row) || view.series_count == 0 || view.first >= view.last {
        return None;
    }
    let visible = view.last - view.first;
    let visible_index = if visible == 1 || view.line_plot.width <= 1 {
        0
    } else {
        let local = usize::from(column - view.line_plot.x);
        let denominator = usize::from(view.line_plot.width - 1);
        (local * (visible - 1) + denominator / 2) / denominator
    };
    Some((
        view.first + visible_index.min(visible - 1),
        view.active_series.min(view.series_count - 1),
    ))
}

fn context_cell_keys(app: &App, dataset: &ChartDataset) -> Vec<(usize, usize)> {
    if !app.selected.is_empty() {
        return app.selected.iter().copied().collect();
    }
    let (first, last) = if app.view.first < app.view.last {
        (app.view.first, app.view.last)
    } else {
        (dataset.row_count().saturating_sub(20), dataset.row_count())
    };
    (first..last)
        .flat_map(|row| (0..dataset.series_count()).map(move |series| (row, series)))
        .collect()
}

fn cell_value(dataset: &ChartDataset, row: usize, series: usize) -> Value {
    let series_spec = &dataset.document.series[series];
    let source_row = &dataset.document.table.rows[row];
    let identifier = dataset
        .document
        .table
        .id_field
        .as_ref()
        .and_then(|field| source_row.get(field))
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "rowIndex": row,
        "id": identifier,
        "category": dataset.categories[row],
        "seriesIndex": series,
        "seriesField": series_spec.field,
        "seriesLabel": series_spec.label,
        "value": dataset.value(row, series).unwrap_or_default()
    })
}

fn visible_range_value(dataset: &ChartDataset, view: &ViewInfo) -> Value {
    if view.first >= view.last || view.last > dataset.row_count() {
        return Value::Null;
    }
    json!({
        "firstIndex": view.first,
        "lastIndexExclusive": view.last,
        "count": view.last - view.first,
        "firstCategory": dataset.categories[view.first],
        "lastCategory": dataset.categories[view.last - 1]
    })
}

fn bar_capacity(
    plot_width: u16,
    series_count: usize,
    bar_width: u16,
    bar_gap: u16,
    group_gap: u16,
) -> usize {
    if series_count == 0 {
        return 0;
    }
    let series_count = series_count as u32;
    let group_width =
        series_count * u32::from(bar_width) + series_count.saturating_sub(1) * u32::from(bar_gap);
    let stride = series_count * u32::from(bar_width + bar_gap) + u32::from(group_gap);
    let width = u32::from(plot_width);
    if width < group_width || stride == 0 {
        return 0;
    }
    (1 + (width - group_width) / stride) as usize
}

fn line_capacity(plot_width: u16, point_spacing: u16) -> usize {
    let usable = plot_width.saturating_sub(10);
    if usable == 0 {
        return 0;
    }
    usize::from(usable / point_spacing.max(1)).max(2)
}

fn line_hit_area(dataset: &ChartDataset, maximum: f64, area: Rect) -> Rect {
    let inner = inset(area, 1);
    let y_label_width = [0.0, maximum / 2.0, maximum]
        .into_iter()
        .map(|value| dataset.format_value(value).chars().count() as u16)
        .max()
        .unwrap_or(0)
        .saturating_add(2)
        .min(inner.width);
    Rect {
        x: inner.x.saturating_add(y_label_width),
        y: inner.y,
        width: inner.width.saturating_sub(y_label_width),
        height: inner.height.saturating_sub(2),
    }
}

fn group_label_width(dataset: &ChartDataset, bar_width: u16, bar_gap: u16) -> usize {
    dataset
        .series_count()
        .saturating_mul(usize::from(bar_width + bar_gap))
        .saturating_sub(usize::from(bar_gap))
}

fn axis_category_labels(dataset: &ChartDataset, first: usize, last: usize) -> Vec<String> {
    if first >= last {
        return vec![String::new(), String::new()];
    }
    if last - first == 1 {
        return vec![
            dataset.categories[first].clone(),
            dataset.categories[first].clone(),
        ];
    }
    let middle = first + (last - first - 1) / 2;
    vec![
        dataset.categories[first].clone(),
        dataset.categories[middle].clone(),
        dataset.categories[last - 1].clone(),
    ]
}

fn inset(area: Rect, margin: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(margin),
        y: area.y.saturating_add(margin),
        width: area.width.saturating_sub(margin.saturating_mul(2)),
        height: area.height.saturating_sub(margin.saturating_mul(2)),
    }
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn short_label(value: &str, width: usize) -> String {
    value.chars().take(width.max(1)).collect()
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
        "chart".to_string()
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
