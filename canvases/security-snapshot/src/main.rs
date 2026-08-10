use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ft_canvas_runtime::{CanvasClient, ClientEvent, IncomingFrame, LaunchConfig};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct Snapshot {
    #[serde(default = "schema_version")]
    schema_version: u8,
    #[serde(rename = "type", default)]
    security_type: Option<String>,
    symbol: String,
    #[serde(alias = "symbol_name")]
    symbol_name: String,
    #[serde(default)]
    as_of: Option<String>,
    #[serde(default = "default_source")]
    source: String,
    #[serde(default, deserialize_with = "number")]
    open: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    high: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    low: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    close: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    prev_close: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    amplitude: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    volume: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    turnover: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    turnover_rate: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate_day5: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate_day10: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate_day20: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate_day60: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    change_rate_ytd: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    market_cap: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    float_a_market_cap: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    shares: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    float_a_shares: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    bvps: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    eps_ttm: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pe_ttm: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    pb: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    ps_ttm: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    roe_ttm: Option<f64>,
    #[serde(default, deserialize_with = "number")]
    bid_ask_ratio: Option<f64>,
    #[serde(default)]
    introduction: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

fn schema_version() -> u8 {
    1
}
fn default_source() -> String {
    "FTShare".into()
}
fn number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(v)) => v.as_f64(),
        Some(Value::String(v)) => v.parse().ok(),
        _ => None,
    })
}

struct App {
    snapshot: Snapshot,
    active: usize,
    status: String,
    should_exit: bool,
}

impl App {
    fn load(value: Value) -> Result<Snapshot, String> {
        let data = source::load(value)?;
        let snapshot: Snapshot = serde_json::from_value(data)
            .map_err(|e| format!("invalid security-snapshot response: {e}"))?;
        if snapshot.schema_version != 1 {
            return Err("schemaVersion must be 1".into());
        }
        if snapshot.symbol.trim().is_empty() || snapshot.symbol_name.trim().is_empty() {
            return Err("FTShare response requires symbol and symbol_name".into());
        }
        Ok(snapshot)
    }
    fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            active: 0,
            status: "←→ section | a attach | Enter analyze | e export | q close".into(),
            should_exit: false,
        }
    }
    fn sections(&self) -> [Value; 4] {
        let s = &self.snapshot;
        [
            json!({"section":"market","symbol":s.symbol,"name":s.symbol_name,"open":s.open,"high":s.high,"low":s.low,"close":s.close,"prevClose":s.prev_close,"change":s.change,"changeRate":s.change_rate,"amplitude":s.amplitude,"volume":s.volume,"turnover":s.turnover,"turnoverRate":s.turnover_rate}),
            json!({"section":"performance","day5":s.change_rate_day5,"day10":s.change_rate_day10,"day20":s.change_rate_day20,"day60":s.change_rate_day60,"ytd":s.change_rate_ytd}),
            json!({"section":"valuation","peTtm":s.pe_ttm,"pb":s.pb,"psTtm":s.ps_ttm,"epsTtm":s.eps_ttm,"bvps":s.bvps,"roeTtm":s.roe_ttm}),
            json!({"section":"capitalization","marketCap":s.market_cap,"floatMarketCap":s.float_a_market_cap,"shares":s.shares,"floatShares":s.float_a_shares,"bidAskRatio":s.bid_ask_ratio}),
        ]
    }
    fn selection(&self) -> Value {
        let sections = self.sections();
        json!({"symbol":self.snapshot.symbol,"name":self.snapshot.symbol_name,"activeSection":sections[self.active],"asOf":self.snapshot.as_of,"source":self.snapshot.source})
    }
}

fn main() {
    if let Err(e) = start() {
        eprintln!("security-snapshot: {e}");
        std::process::exit(1)
    }
}
fn start() -> Result<(), String> {
    let path = parse_launch_file()?;
    let launch = LaunchConfig::read(&path)?;
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
            .draw(|frame| match app.as_ref() {
                Some(app) => draw(frame, app),
                None => draw_status(frame, &status),
            })
            .map_err(|e| e.to_string())?;
        while let Ok(e) = receiver.try_recv() {
            handle_client(&mut app, &mut status, &mut should_exit, client, e)?;
        }
        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = event::read().map_err(|e| e.to_string())? {
                if k.kind == KeyEventKind::Press {
                    if let Some(app) = app.as_mut() {
                        handle_key(app, client, launch, k.code)?;
                        should_exit = app.should_exit;
                    } else if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
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
                .title(" Security Snapshot "),
        ),
        frame.area(),
    );
}
fn draw(frame: &mut ratatui::Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(12),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let s = &app.snapshot;
    let headline = format!(
        "{}  {}   {}  {}\nAs of {} · {}",
        price(s.close),
        percent(s.change_rate),
        range(s.low, s.high),
        amount(s.turnover),
        s.as_of.as_deref().unwrap_or("unknown"),
        s.source
    );
    frame.render_widget(
        Paragraph::new(headline)
            .style(change_style(s.change_rate))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} {} ", s.symbol_name, s.symbol)),
            ),
        header,
    );
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [market, performance] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);
    let [valuation, capital] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(bottom);
    panel(
        frame,
        market,
        "Market",
        app.active == 0,
        vec![
            ("Open", price(s.open)),
            ("High / Low", range(s.low, s.high)),
            ("Prev close", price(s.prev_close)),
            ("Amplitude", percent(s.amplitude)),
            ("Turnover rate", percent(s.turnover_rate)),
        ],
    );
    panel(
        frame,
        performance,
        "Performance",
        app.active == 1,
        vec![
            ("5 days", percent(s.change_rate_day5)),
            ("10 days", percent(s.change_rate_day10)),
            ("20 days", percent(s.change_rate_day20)),
            ("60 days", percent(s.change_rate_day60)),
            ("YTD", percent(s.change_rate_ytd)),
        ],
    );
    panel(
        frame,
        valuation,
        "Valuation & quality",
        app.active == 2,
        vec![
            ("PE TTM", decimal(s.pe_ttm)),
            ("PB", decimal(s.pb)),
            ("PS TTM", decimal(s.ps_ttm)),
            ("EPS TTM", decimal(s.eps_ttm)),
            ("ROE TTM", percent(s.roe_ttm)),
        ],
    );
    panel(
        frame,
        capital,
        "Capitalization",
        app.active == 3,
        vec![
            ("Market cap", amount(s.market_cap)),
            ("Float cap", amount(s.float_a_market_cap)),
            ("Shares", amount(s.shares)),
            ("Float shares", amount(s.float_a_shares)),
            ("Bid/ask ratio", percent(s.bid_ask_ratio)),
        ],
    );
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::new().fg(Color::Gray)),
        footer,
    );
}
fn panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    active: bool,
    values: Vec<(&str, String)>,
) {
    let rows = values
        .into_iter()
        .map(|(k, v)| Row::new([Cell::from(k), Cell::from(v)]));
    let border = if active { Color::Cyan } else { Color::DarkGray };
    let table = Table::new(
        rows,
        [Constraint::Percentage(55), Constraint::Percentage(45)],
    )
    .column_spacing(1)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border))
            .title(format!(" {title} ")),
    )
    .row_highlight_style(Style::new().add_modifier(Modifier::BOLD));
    frame.render_widget(table, area);
}
fn handle_key(
    app: &mut App,
    client: &CanvasClient,
    launch: &LaunchConfig,
    key: KeyCode,
) -> Result<(), String> {
    match key{KeyCode::Char('q')|KeyCode::Esc=>{client.request_close()?;app.should_exit=true},KeyCode::Left=>app.active=app.active.saturating_sub(1),KeyCode::Right|KeyCode::Tab=>app.active=(app.active+1)%4,KeyCode::Char('a')=>client.send_context("security snapshot section",format!("{} {} snapshot section",app.snapshot.symbol_name,app.snapshot.symbol),app.selection())?,KeyCode::Enter=>client.send_action("analyze security snapshot",format!("Analyze {} {} using the selected snapshot section. Explain performance, valuation, liquidity, risks, and important caveats.",app.snapshot.symbol_name,app.snapshot.symbol),app.selection())?,KeyCode::Char('e')=>export(app,client,launch)?,_=>{}}
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
                Ok(snapshot) => {
                    let title = format!("{} {}", snapshot.symbol_name, snapshot.symbol);
                    *status = format!("Loaded {title}");
                    *app = Some(App::new(snapshot));
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
                |app| json!({"symbol":app.snapshot.symbol,"activeSection":app.active,"asOf":app.snapshot.as_of,"loading":false}),
            ),
        )?,
        "request.selection" => client.send_rpc_ok(
            required(&frame)?,
            app.as_ref().map_or(Value::Null, App::selection),
        )?,
        "request.content" => client.send_rpc_error(
            required(&frame)?,
            "security-snapshot does not expose editable content",
        )?,
        "ping" => client.send_pong()?,
        "close" => *should_exit = true,
        _ => {}
    }
    Ok(())
}
fn export(app: &App, client: &CanvasClient, launch: &LaunchConfig) -> Result<(), String> {
    let path = launch.runtime_dir.join(format!(
        "{}-snapshot-{}.json",
        app.snapshot.symbol.replace('.', "-"),
        now_millis()
    ));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&app.snapshot).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    client.send_artifact(
        "security snapshot",
        &path,
        format!("Exported snapshot for {}.", app.snapshot.symbol),
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
    Err("usage: security-snapshot --launch-file <launch.json>".into())
}
fn price(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".into())
}
fn decimal(v: Option<f64>) -> String {
    price(v)
}
fn percent(v: Option<f64>) -> String {
    v.map(|v| format!("{:+.2}%", v * 100.0))
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
fn range(low: Option<f64>, high: Option<f64>) -> String {
    format!("L {} / H {}", price(low), price(high))
}
fn change_style(v: Option<f64>) -> Style {
    match v {
        Some(v) if v > 0.0 => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        Some(v) if v < 0.0 => Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
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
    fn accepts_raw_ftshare_snapshot_shape() {
        let snapshot: Snapshot = serde_json::from_value(
            json!({"symbol":"600519.SH","symbol_name":"贵州茅台","close":"1400.97","pe_ttm":19.48}),
        )
        .unwrap();
        assert_eq!(snapshot.close, Some(1400.97));
        assert_eq!(snapshot.pe_ttm, Some(19.48));
    }
    #[test]
    fn rejects_inline_snapshot() {
        assert!(App::load(json!({"symbol":"600519.SH","symbol_name":"贵州茅台"})).is_err());
    }
}
mod source;
