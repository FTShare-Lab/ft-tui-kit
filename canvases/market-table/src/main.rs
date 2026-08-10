use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ft_canvas_runtime::{CanvasClient, ClientEvent, IncomingFrame, LaunchConfig};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default = "schema_version")]
    schema_version: u8,
    #[serde(default = "default_title")]
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    as_of: Option<String>,
    #[serde(default = "default_source")]
    source: String,
    items: Vec<Quote>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Quote {
    symbol: String,
    name: String,
    #[serde(default, deserialize_with = "number")]
    close: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    turnover: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    turnover_rate: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pe_ttm: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    market_cap: Option<f64>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

fn schema_version() -> u8 {
    1
}
fn default_title() -> String {
    "Market Quotes".into()
}
fn default_source() -> String {
    "FTShare".into()
}

fn number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(value)) => value.as_f64(),
        Some(Value::String(value)) => value.parse().ok(),
        _ => None,
    })
}

struct App {
    document: Document,
    cursor: usize,
    selected: BTreeSet<usize>,
    offset: usize,
    visible: usize,
    status: String,
    should_exit: bool,
}

impl App {
    fn load(value: Value) -> Result<Document, String> {
        let data = source::load(value)?;
        let document: Document = serde_json::from_value(data)
            .map_err(|error| format!("invalid market-table document: {error}"))?;
        if document.schema_version != 1 {
            return Err("schemaVersion must be 1".into());
        }
        if document.items.is_empty() {
            return Err("items must contain at least one quote".into());
        }
        if document.items.len() > 10_000 {
            return Err("items cannot exceed 10000 rows".into());
        }
        for (index, item) in document.items.iter().enumerate() {
            if item.symbol.trim().is_empty() || item.name.trim().is_empty() {
                return Err(format!("items[{index}] requires symbol and name"));
            }
        }
        Ok(document)
    }

    fn new(document: Document) -> Self {
        Self {
            document,
            cursor: 0,
            selected: BTreeSet::new(),
            offset: 0,
            visible: 1,
            status: "↑↓ navigate | Space select | a attach | Enter analyze | e export | q close"
                .into(),
            should_exit: false,
        }
    }

    fn current(&self) -> &Quote {
        &self.document.items[self.cursor]
    }
    fn move_by(&mut self, delta: isize) {
        let last = self.document.items.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.visible {
            self.offset = self.cursor + 1 - self.visible;
        }
    }
    fn selection_value(&self) -> Value {
        let rows: Vec<&Quote> = self
            .selected
            .iter()
            .filter_map(|i| self.document.items.get(*i))
            .collect();
        json!({ "title": self.document.title, "source": self.document.source, "asOf": self.document.as_of,
            "cursor": self.cursor, "current": self.current(), "selected": rows })
    }
    fn context_value(&self) -> Value {
        if self.selected.is_empty() {
            json!([self.current()])
        } else {
            Value::Array(
                self.selected
                    .iter()
                    .filter_map(|i| self.document.items.get(*i))
                    .map(|q| json!(q))
                    .collect(),
            )
        }
    }
}

fn main() {
    if let Err(error) = start() {
        eprintln!("market-table: {error}");
        std::process::exit(1);
    }
}

fn start() -> Result<(), String> {
    let launch_path = parse_launch_file()?;
    let launch = LaunchConfig::read(&launch_path)?;
    let (sender, receiver) = mpsc::channel();
    let client = CanvasClient::connect(launch.clone(), sender)?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &launch, &client, receiver);
    ratatui::restore();
    client.close();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    launch: &LaunchConfig,
    client: &CanvasClient,
    receiver: mpsc::Receiver<ClientEvent>,
) -> Result<(), String> {
    let mut app: Option<App> = None;
    let mut status = "Waiting for Canvas init...".to_string();
    let mut should_exit = false;
    while !should_exit {
        terminal
            .draw(|frame| match app.as_mut() {
                Some(app) => draw(frame, app),
                None => draw_status(frame, &status),
            })
            .map_err(|e| e.to_string())?;
        while let Ok(event) = receiver.try_recv() {
            handle_client(&mut app, &mut status, &mut should_exit, client, event)?;
        }
        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                if key.kind == KeyEventKind::Press {
                    if let Some(app) = app.as_mut() {
                        handle_key(app, client, launch, key.code)?;
                        should_exit = app.should_exit;
                    } else if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                        client.request_close()?;
                        should_exit = true;
                    }
                }
            }
        }
    }
    Ok(())
}

fn draw_status(frame: &mut ratatui::Frame, status: &str) {
    frame.render_widget(
        Paragraph::new(status).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Market Table "),
        ),
        frame.area(),
    );
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let [header, table_area, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let subtitle = format!(
        "{}  {}  {} rows",
        app.document.subtitle.as_deref().unwrap_or(""),
        app.document.as_of.as_deref().unwrap_or(""),
        app.document.items.len()
    );
    frame.render_widget(
        Paragraph::new(subtitle).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", app.document.title)),
        ),
        header,
    );
    app.visible = table_area.height.saturating_sub(3).max(1) as usize;
    if app.cursor >= app.offset + app.visible {
        app.offset = app.cursor + 1 - app.visible;
    }
    let rows = app
        .document
        .items
        .iter()
        .enumerate()
        .skip(app.offset)
        .take(app.visible)
        .map(|(index, q)| {
            let marker = if app.selected.contains(&index) {
                "●"
            } else {
                " "
            };
            Row::new(vec![
                Cell::from(marker),
                Cell::from(q.symbol.clone()),
                Cell::from(q.name.clone()),
                Cell::from(price(q.close)),
                Cell::from(percent(q.change_rate, true)).style(change_style(q.change_rate)),
                Cell::from(amount(q.turnover)),
                Cell::from(percent(q.turnover_rate, true)),
                Cell::from(decimal(q.pe_ttm, 2)),
                Cell::from(amount(q.market_cap)),
            ])
        });
    let widths = [
        Constraint::Length(2),
        Constraint::Length(13),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Length(11),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Min(10),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new([
                "",
                "Symbol",
                "Name",
                "Last",
                "Change",
                "Turnover",
                "Turn%",
                "PE",
                "Market Cap",
            ])
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ")
        .block(Block::default().borders(Borders::ALL));
    let mut state =
        TableState::default().with_selected(Some(app.cursor.saturating_sub(app.offset)));
    frame.render_stateful_widget(table, table_area, &mut state);
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::new().fg(Color::Gray)),
        footer,
    );
}

fn handle_key(
    app: &mut App,
    client: &CanvasClient,
    launch: &LaunchConfig,
    key: KeyCode,
) -> Result<(), String> {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            client.request_close()?;
            app.should_exit = true;
        }
        KeyCode::Up => app.move_by(-1),
        KeyCode::Down => app.move_by(1),
        KeyCode::PageUp => app.move_by(-(app.visible as isize)),
        KeyCode::PageDown => app.move_by(app.visible as isize),
        KeyCode::Home => app.move_by(-(app.document.items.len() as isize)),
        KeyCode::End => app.move_by(app.document.items.len() as isize),
        KeyCode::Char(' ') => {
            if !app.selected.insert(app.cursor) {
                app.selected.remove(&app.cursor);
            }
            send_selection(app, client)?;
        }
        KeyCode::Char('c') => {
            app.selected.clear();
            send_selection(app, client)?;
        }
        KeyCode::Char('a') => send_context(app, client)?,
        KeyCode::Enter => send_action(app, client)?,
        KeyCode::Char('e') => export(app, client, launch)?,
        _ => {}
    }
    Ok(())
}

fn handle_client(
    app: &mut Option<App>,
    status: &mut String,
    should_exit: &mut bool,
    client: &CanvasClient,
    event: ClientEvent,
) -> Result<(), String> {
    let frame = match event {
        ClientEvent::Frame(frame) => frame,
        ClientEvent::Disconnected { channel } => {
            *status = format!("Canvas {channel} socket disconnected");
            *should_exit = true;
            return Ok(());
        }
        ClientEvent::Error(error) => {
            *status = error;
            return Ok(());
        }
    };
    if frame.channel == "event" {
        return Ok(());
    }
    match frame.frame_type.as_str() {
        "init" | "update" => {
            let value = frame
                .payload
                .get("config")
                .cloned()
                .ok_or("frame is missing payload.config")?;
            match App::load(value) {
                Ok(document) => {
                    *status = format!("Loaded {} rows", document.items.len());
                    let title = document.title.clone();
                    *app = Some(App::new(document));
                    client.send_ready(Some(&title), frame.request_id.as_deref())?;
                }
                Err(error) => {
                    *status = error.clone();
                    client.send_config_error(&error, frame.request_id.as_deref())?;
                }
            }
        }
        "request.state" => client.send_rpc_ok(
            required(&frame)?,
            app.as_ref().map_or_else(
                || json!({"status":status,"loading":true}),
                |app| json!({"cursor":app.cursor,"offset":app.offset,"visible":app.visible,"rowCount":app.document.items.len(),"loading":false}),
            ),
        )?,
        "request.selection" => client.send_rpc_ok(
            required(&frame)?,
            app.as_ref().map_or(Value::Null, App::selection_value),
        )?,
        "request.content" => client.send_rpc_error(required(&frame)?, "market-table does not expose editable content")?,
        "ping" => client.send_pong()?, "close" => *should_exit = true, _ => {}
    }
    Ok(())
}

fn send_selection(app: &App, client: &CanvasClient) -> Result<(), String> {
    client.send_selection(
        "market rows",
        format!("Selected {} market row(s).", app.selected.len()),
        app.selection_value(),
    )
}
fn send_context(app: &App, client: &CanvasClient) -> Result<(), String> {
    client.send_context(
        "market table context",
        format!(
            "{} market row(s) from {}.",
            if app.selected.is_empty() {
                1
            } else {
                app.selected.len()
            },
            app.document.source
        ),
        app.context_value(),
    )
}
fn send_action(app: &App, client: &CanvasClient) -> Result<(), String> {
    client.send_action("analyze market rows", "Analyze the selected market securities. Compare price performance, liquidity, valuation, size, and notable outliers.".into(), app.context_value())
}
fn export(app: &App, client: &CanvasClient, launch: &LaunchConfig) -> Result<(), String> {
    let path = launch
        .runtime_dir
        .join(format!("market-table-{}.json", now_millis()));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&app.context_value()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    client.send_artifact(
        "market table rows",
        &path,
        "Exported selected market rows.".into(),
    )
}
fn required(frame: &IncomingFrame) -> Result<&str, String> {
    frame
        .request_id
        .as_deref()
        .ok_or_else(|| "requestId is required".into())
}
fn parse_launch_file() -> Result<PathBuf, String> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--launch-file" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or("--launch-file requires a path".into());
        }
    }
    Err("usage: market-table --launch-file <launch.json>".into())
}
fn price(v: Option<f64>) -> String {
    decimal(v, 2)
}
fn decimal(v: Option<f64>, places: usize) -> String {
    v.map(|v| format!("{v:.places$}"))
        .unwrap_or_else(|| "—".into())
}
fn percent(v: Option<f64>, fractional: bool) -> String {
    v.map(|v| format!("{:+.2}%", if fractional { v * 100.0 } else { v }))
        .unwrap_or_else(|| "—".into())
}
fn amount(v: Option<f64>) -> String {
    match v {
        Some(v) if v.abs() >= 1e12 => format!("{:.2}T", v / 1e12),
        Some(v) if v.abs() >= 1e9 => format!("{:.2}B", v / 1e9),
        Some(v) if v.abs() >= 1e6 => format!("{:.2}M", v / 1e6),
        Some(v) if v.abs() >= 1e3 => format!("{:.2}K", v / 1e3),
        Some(v) => format!("{v:.0}"),
        None => "—".into(),
    }
}
fn change_style(v: Option<f64>) -> Style {
    match v {
        Some(v) if v > 0.0 => Style::new().fg(Color::Red),
        Some(v) if v < 0.0 => Style::new().fg(Color::Green),
        _ => Style::new(),
    }
}
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_ftshare_numeric_strings() {
        let data = json!({"schemaVersion":1,"items":[{"symbol":"600000.XSHG","name":"浦发银行","close":"9.32","change_rate":-0.0053}]});
        let document: Document = serde_json::from_value(data).unwrap();
        assert_eq!(document.items[0].close, Some(9.32));
    }
    #[test]
    fn rejects_inline_rows() {
        assert!(App::load(json!({"schemaVersion":1,"items":[]})).is_err());
    }
}
mod source;
