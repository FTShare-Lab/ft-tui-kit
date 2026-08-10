mod config;
mod data;

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use candlesticks_chart::{
    Candle, CandleSeries, CandlestickChart, PriceAxis, TimeAxis, ValueAxis, Volume, VolumeChart,
    VolumeSeries, price_bounds,
};
use chrono::{TimeZone, Utc};
use config::{IntervalUnit, RendererConfig};
use data::{MarketBar, MarketDataset, load_market_data};
use ft_canvas_runtime::{CanvasClient, ClientEvent, IncomingFrame, LaunchConfig};
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::{Value, json};

const AXIS_WIDTH: u16 = 8;
const TIME_AXIS_HEIGHT: u16 = 1;
const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(120);
const ZOOM_LEVELS: [(f64, f64); 6] = [
    (1.0, 0.0),
    (1.0, 1.0),
    (2.0, 1.0),
    (3.0, 1.0),
    (5.0, 1.0),
    (8.0, 2.0),
];

fn main() {
    if let Err(error) = start() {
        eprintln!("candlesticks: {error}");
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
    Err("usage: candlesticks --launch-file <launch.json>".to_string())
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
    result: Result<MarketDataset, LoadFailure>,
}

#[derive(Debug)]
enum LoadFailure {
    Config(String),
    Data(String),
}

#[derive(Debug, Clone, Copy)]
struct ViewInfo {
    price_plot: Rect,
    volume_plot: Rect,
    first: usize,
    last: usize,
    width: f64,
    gap: f64,
    price_span: Option<(f64, f64)>,
}

impl ViewInfo {
    fn empty() -> Self {
        Self {
            price_plot: Rect::new(0, 0, 0, 0),
            volume_plot: Rect::new(0, 0, 0, 0),
            first: 0,
            last: 0,
            width: 1.0,
            gap: 1.0,
            price_span: None,
        }
    }
}

struct App {
    right: usize,
    zoom: usize,
    price_offset: f64,
    highlighted: BTreeSet<usize>,
    dataset: Option<MarketDataset>,
    candles: Vec<Candle>,
    volumes: Vec<Volume>,
    labels: Vec<String>,
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
            zoom: 1,
            price_offset: 0.0,
            highlighted: BTreeSet::new(),
            dataset: None,
            candles: Vec::new(),
            volumes: Vec::new(),
            labels: Vec::new(),
            view: ViewInfo::empty(),
            status: "Loading market data...".to_string(),
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
        self.candles.len().min(self.volumes.len())
    }

    fn width_gap(&self) -> (f64, f64) {
        ZOOM_LEVELS[self.zoom]
    }

    fn clamp(&mut self) {
        let total = self.total();
        self.right = self.right.min(total);
        if total > 0 && self.right == 0 {
            self.right = 1;
        }
        self.highlighted.retain(|index| *index < total);
    }

    fn pan_left(&mut self) {
        self.right = self.right.saturating_sub(1).max(1);
        self.changed_view();
    }

    fn pan_right(&mut self) {
        self.right = (self.right + 1).min(self.total());
        self.changed_view();
    }

    fn pan_price(&mut self, rows: f64) {
        if let Some((lo, hi)) = self.view.price_span {
            let step = ((hi - lo).abs() * 0.10).max(0.01);
            self.price_offset += rows * step;
            self.changed_view();
        }
    }

    fn zoom_in(&mut self) {
        let next = (self.zoom + 1).min(ZOOM_LEVELS.len() - 1);
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

    fn toggle_highlight(&mut self, index: usize) {
        if !self.highlighted.insert(index) {
            self.highlighted.remove(&index);
        }
        self.dirty = true;
        self.state_dirty = true;
    }

    fn clear_selection(&mut self) {
        self.highlighted.clear();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn changed_view(&mut self) {
        self.clamp();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn apply_dataset(&mut self, dataset: MarketDataset) {
        let old_total = self.total();
        let was_at_right = old_total == 0 || self.right == old_total;
        let old_tag = self
            .dataset
            .as_ref()
            .map(|dataset| dataset.config.tag.clone());
        let selected_times: HashSet<i64> = self
            .dataset
            .as_ref()
            .map(|dataset| {
                self.highlighted
                    .iter()
                    .filter_map(|index| dataset.bars.get(*index))
                    .map(|bar| bar.ts_millis)
                    .collect()
            })
            .unwrap_or_default();

        self.candles = dataset
            .bars
            .iter()
            .map(|bar| Candle::new(bar.open, bar.high, bar.low, bar.close))
            .collect();
        self.volumes = dataset
            .bars
            .iter()
            .zip(self.candles.iter())
            .map(|(bar, candle)| Volume::new(bar.volume).with_direction(candle.direction()))
            .collect();
        self.labels = dataset
            .bars
            .iter()
            .map(|bar| format_timestamp(bar.ts_millis, dataset.config.interval_unit))
            .collect();

        let same_tag = old_tag.as_deref() == Some(dataset.config.tag.as_str());
        self.highlighted = if same_tag {
            dataset
                .bars
                .iter()
                .enumerate()
                .filter(|(_, bar)| selected_times.contains(&bar.ts_millis))
                .map(|(index, _)| index)
                .collect()
        } else {
            BTreeSet::new()
        };
        self.dataset = Some(dataset);
        self.right = if was_at_right {
            self.total()
        } else {
            self.right.min(self.total())
        };
        if !same_tag {
            self.zoom = 1;
            self.price_offset = 0.0;
        }
        self.loading = false;
        self.status = self
            .dataset
            .as_ref()
            .map(|dataset| {
                format!(
                    "{} bars | {} | a attach | Enter analyze | e export | c clear | q close",
                    dataset.bars.len(),
                    dataset.source
                )
            })
            .unwrap_or_default();
        self.clamp();
        self.dirty = true;
        self.state_dirty = true;
    }

    fn visible_bars(&self) -> &[MarketBar] {
        let Some(dataset) = self.dataset.as_ref() else {
            return &[];
        };
        if self.view.first > self.view.last || self.view.last > dataset.bars.len() {
            return &[];
        }
        &dataset.bars[self.view.first..self.view.last]
    }

    fn selected_bars(&self) -> Vec<&MarketBar> {
        let Some(dataset) = self.dataset.as_ref() else {
            return Vec::new();
        };
        self.highlighted
            .iter()
            .filter_map(|index| dataset.bars.get(*index))
            .collect()
    }

    fn selection_value(&self) -> Value {
        let Some(dataset) = self.dataset.as_ref() else {
            return Value::Null;
        };
        let bars = self.selected_bars();
        json!({
            "tag": dataset.config.tag,
            "timeframe": dataset.config.timeframe(),
            "indices": self.highlighted,
            "candles": bars
        })
    }

    fn state_value(&self, key: Option<&str>) -> Result<Value, String> {
        let Some(dataset) = self.dataset.as_ref() else {
            return Ok(json!({ "status": self.status, "loading": self.loading }));
        };
        let visible_range = range_value(self.visible_bars(), self.view.first, self.view.last);
        let selected = self.selected_bars();
        let selected_range = range_value_refs(&selected);
        let all = json!({
            "symbol": dataset.config.tag,
            "timeframe": dataset.config.timeframe(),
            "visibleRange": visible_range.clone(),
            "selectedRange": selected_range.clone(),
            "zoom": self.zoom,
            "priceOffset": self.price_offset,
            "source": dataset.source,
            "loading": self.loading
        });
        match key {
            None => Ok(all),
            Some("symbol") => Ok(json!(dataset.config.tag)),
            Some("timeframe") => Ok(json!(dataset.config.timeframe())),
            Some("visibleRange") => Ok(visible_range),
            Some("selectedRange") => Ok(selected_range),
            Some("zoom") => Ok(json!(self.zoom)),
            Some("priceOffset") => Ok(json!(self.price_offset)),
            Some("source") => Ok(json!(dataset.source)),
            Some(other) => Err(format!("unknown state key: {other}")),
        }
    }

    fn ready_title(&self) -> Option<String> {
        self.dataset
            .as_ref()
            .map(|dataset| format!("{} {}", dataset.config.tag, dataset.config.timeframe()))
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

        if app.state_dirty && last_state_emit.elapsed() >= STATE_EMIT_INTERVAL {
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
    app.status = "Loading market data...".to_string();
    app.dirty = true;
    thread::spawn(move || {
        let result = match RendererConfig::from_value(value) {
            Ok(config) => load_market_data(config).map_err(LoadFailure::Data),
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
        Err(LoadFailure::Data(error)) => {
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
        match frame.frame_type.as_str() {
            "event.nack" | "backpressure" => {
                app.status = format!("Canvas host: {}", frame.payload);
                app.dirty = true;
            }
            _ => {}
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
                "candlesticks renderer does not expose editable content",
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
            KeyCode::Up => app.pan_price(-1.0),
            KeyCode::Down => app.pan_price(1.0),
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
            if let Some(index) = hit_bar(&app.view, mouse.column, mouse.row) {
                app.toggle_highlight(index);
                send_selection(app, client)?;
            }
        }
        MouseEventKind::ScrollUp => app.zoom_in(),
        MouseEventKind::ScrollDown => app.zoom_out(),
        _ => {}
    }
    Ok(())
}

fn send_selection(app: &App, client: &CanvasClient) -> Result<(), String> {
    let count = app.highlighted.len();
    let text = if count == 0 {
        "The chart selection was cleared.".to_string()
    } else {
        format!("The user selected {count} candlestick(s) on the candlesticks chart.")
    };
    client.send_selection("selected candlesticks", text, app.selection_value())
}

fn send_context(app: &App, client: &CanvasClient) -> Result<(), String> {
    let (text, data) = summarized_context(app)?;
    client.send_context("candlesticks chart context", text, data)
}

fn send_analysis_action(app: &App, client: &CanvasClient) -> Result<(), String> {
    let (summary, data) = summarized_context(app)?;
    let prompt = format!(
        "Analyze the selected candlestick chart range. Identify trend, support/resistance, volatility, and notable volume behavior. Chart summary: {summary}"
    );
    client.send_action("analyze chart range", prompt, data)
}

fn summarized_context(app: &App) -> Result<(String, Value), String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "market data is not loaded yet".to_string())?;
    let selected = app.selected_bars();
    let bars: Vec<&MarketBar> = if selected.is_empty() {
        app.visible_bars().iter().collect()
    } else {
        selected
    };
    if bars.is_empty() {
        return Err("no visible candlesticks are available".to_string());
    }
    let first = bars.first().unwrap();
    let last = bars.last().unwrap();
    let high = bars.iter().map(|bar| bar.high).fold(f64::MIN, f64::max);
    let low = bars.iter().map(|bar| bar.low).fold(f64::MAX, f64::min);
    let volume: f64 = bars.iter().map(|bar| bar.volume).sum();
    let summary = format!(
        "{} {}, {} bars from {} to {}, open {:.4}, close {:.4}, high {:.4}, low {:.4}, total volume {:.0}",
        dataset.config.tag,
        dataset.config.timeframe(),
        bars.len(),
        first.ts_millis,
        last.ts_millis,
        first.open,
        last.close,
        high,
        low,
        volume
    );
    Ok((
        summary,
        json!({
            "tag": dataset.config.tag,
            "timeframe": dataset.config.timeframe(),
            "count": bars.len(),
            "startTsMillis": first.ts_millis,
            "endTsMillis": last.ts_millis,
            "open": first.open,
            "close": last.close,
            "high": high,
            "low": low,
            "volume": volume
        }),
    ))
}

fn export_artifact(app: &App, client: &CanvasClient, launch: &LaunchConfig) -> Result<(), String> {
    let dataset = app
        .dataset
        .as_ref()
        .ok_or_else(|| "market data is not loaded yet".to_string())?;
    let selected = app.selected_bars();
    let bars: Vec<&MarketBar> = if selected.is_empty() {
        app.visible_bars().iter().collect()
    } else {
        selected
    };
    if bars.is_empty() {
        return Err("no candlesticks are available to export".to_string());
    }

    let path = launch.runtime_dir.join(format!(
        "{}-candlesticks-{}.json",
        dataset.config.tag.replace('.', "-"),
        now_millis()
    ));
    let artifact = json!({
        "tag": dataset.config.tag,
        "timeframe": dataset.config.timeframe(),
        "source": dataset.source,
        "candles": bars
    });
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|error| format!("cannot encode candlestick artifact: {error}"))?;
    std::fs::write(&path, bytes)
        .map_err(|error| format!("cannot write artifact {}: {error}", path.display()))?;
    client.send_artifact(
        "candlestick data",
        &path,
        format!(
            "Exported {} candlesticks for {}.",
            bars.len(),
            dataset.config.tag
        ),
    )
}

fn emit_state(app: &mut App, client: &CanvasClient) -> Result<(), String> {
    if app.dataset.is_none() {
        return Ok(());
    }
    client.send_state("visibleRange", app.state_value(Some("visibleRange"))?)?;
    client.send_state("selectedRange", app.state_value(Some("selectedRange"))?)?;
    client.send_state("zoom", app.state_value(Some("zoom"))?)?;
    app.state_dirty = false;
    Ok(())
}

fn draw(frame: &mut Frame, app: &App) -> ViewInfo {
    let [content_area, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let status_style = Style::new()
        .fg(if app.loading {
            Color::Yellow
        } else {
            Color::Gray
        })
        .bg(Color::Rgb(13, 17, 23));
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(status_style),
        status_area,
    );

    if app.dataset.is_none() {
        frame.render_widget(
            Paragraph::new(app.status.as_str())
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Candlesticks Canvas "),
                ),
            content_area,
        );
        return ViewInfo::empty();
    }

    let [price_area, volume_area] =
        Layout::vertical([Constraint::Min(8), Constraint::Percentage(30)]).areas(content_area);
    let price_plot = chart_plot_area(price_area);
    let volume_plot = chart_plot_area(volume_area);
    let bull = Color::Rgb(38, 166, 154);
    let bear = Color::Rgb(239, 83, 80);
    let highlight = Color::Rgb(255, 214, 102);
    let axis_style = Style::new().fg(Color::Rgb(120, 123, 134));
    let base = Style::new().bg(Color::Rgb(13, 17, 23));
    let (width, gap) = app.width_gap();
    let total = app.total();
    let capacity = candle_capacity(price_plot.width, width, gap).min(total);
    let last = app.right.min(total);
    let first = if capacity == 0 {
        last
    } else {
        last.saturating_sub(capacity)
    };
    let candles = &app.candles[first..last];
    let volumes = &app.volumes[first..last];
    let labels = &app.labels[first..last];
    let highlighted: Vec<usize> = app
        .highlighted
        .range(first..last)
        .map(|index| index - first)
        .collect();
    let price_span = price_bounds(candles);
    let price_axis = match price_span {
        Some((lo, hi)) => PriceAxis::default()
            .style(axis_style)
            .bounds([lo + app.price_offset, hi + app.price_offset]),
        None => PriceAxis::default().style(axis_style),
    };
    let dataset = app.dataset.as_ref().unwrap();
    let title = format!(
        " {} {} | arrows pan | wheel zoom | click select ",
        dataset.config.tag,
        dataset.config.timeframe()
    );

    let candle_series = CandleSeries::new(candles)
        .width(width)
        .gap(gap)
        .bull_style(bull)
        .bear_style(bear)
        .wick_style(Color::Rgb(110, 116, 130))
        .highlighted(&highlighted)
        .highlight_style(highlight);
    let price_chart = CandlestickChart::new(candle_series)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(base)
        .price_axis(price_axis)
        .time_axis(TimeAxis::default().style(axis_style).labels(labels));

    let volume_series = VolumeSeries::new(volumes)
        .width(width)
        .gap(gap)
        .bull_style(bull)
        .bear_style(bear)
        .highlighted(&highlighted)
        .highlight_style(highlight);
    let volume_chart = VolumeChart::new(volume_series)
        .block(Block::default().borders(Borders::ALL).title(" Volume "))
        .style(base)
        .value_axis(ValueAxis::default().style(axis_style))
        .time_axis(TimeAxis::default().style(axis_style).labels(labels));

    frame.render_widget(&price_chart, price_area);
    frame.render_widget(&volume_chart, volume_area);

    ViewInfo {
        price_plot,
        volume_plot,
        first,
        last,
        width,
        gap,
        price_span,
    }
}

fn hit_bar(view: &ViewInfo, column: u16, row: u16) -> Option<usize> {
    let plot = if contains(view.price_plot, column, row) {
        view.price_plot
    } else if contains(view.volume_plot, column, row) {
        view.volume_plot
    } else {
        return None;
    };
    let visible = view.last.saturating_sub(view.first);
    let local_col = column - plot.x;
    let visible_index = col_to_visible_index(local_col, visible, view.width, view.gap)?;
    Some(view.first + visible_index)
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn col_to_visible_index(col: u16, visible: usize, width: f64, gap: f64) -> Option<usize> {
    let slot = width + gap;
    if slot <= 0.0 {
        return None;
    }
    let center = f64::from(col) + 0.5;
    let index = (center / slot).floor() as usize;
    let within = center - index as f64 * slot;
    (within < width && index < visible).then_some(index)
}

fn candle_capacity(plot_width: u16, width: f64, gap: f64) -> usize {
    let slot = width + gap;
    if slot <= 0.0 {
        return 0;
    }
    ((f64::from(plot_width) + gap) / slot).floor() as usize
}

fn chart_plot_area(area: Rect) -> Rect {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if inner.width <= AXIS_WIDTH || inner.height <= TIME_AXIS_HEIGHT {
        return Rect::new(inner.x, inner.y, 0, 0);
    }
    Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width - AXIS_WIDTH,
        height: inner.height - TIME_AXIS_HEIGHT,
    }
}

fn range_value(bars: &[MarketBar], first: usize, last: usize) -> Value {
    let Some(start) = bars.first() else {
        return Value::Null;
    };
    let end = bars.last().unwrap();
    json!({
        "firstIndex": first,
        "lastIndexExclusive": last,
        "startTsMillis": start.ts_millis,
        "endTsMillis": end.ts_millis,
        "count": bars.len()
    })
}

fn range_value_refs(bars: &[&MarketBar]) -> Value {
    let Some(start) = bars.first() else {
        return Value::Null;
    };
    let end = bars.last().unwrap();
    json!({
        "startTsMillis": start.ts_millis,
        "endTsMillis": end.ts_millis,
        "count": bars.len()
    })
}

fn format_timestamp(ts_millis: i64, unit: IntervalUnit) -> String {
    let Some(time) = Utc.timestamp_millis_opt(ts_millis).single() else {
        return ts_millis.to_string();
    };
    match unit {
        IntervalUnit::Minute => time.format("%m-%d %H:%M").to_string(),
        IntervalUnit::Day => time.format("%Y-%m-%d").to_string(),
        IntervalUnit::Week | IntervalUnit::Month | IntervalUnit::Year => {
            time.format("%Y-%m").to_string()
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
