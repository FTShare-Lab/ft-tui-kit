//! Runtime-side Canvas v2 socket client shared by independent native renderers.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const PROTOCOL_VERSION: u8 = 2;
const MAX_FRAME_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub version: u8,
    pub widget_id: String,
    pub kind: String,
    pub scenario: String,
    pub title: Option<String>,
    pub token: String,
    pub runtime_dir: PathBuf,
    pub control_socket_path: PathBuf,
    pub event_socket_path: PathBuf,
    pub config_path: PathBuf,
}

impl LaunchConfig {
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path)
            .map_err(|error| format!("cannot open launch file {}: {error}", path.display()))?;
        let launch: Self = serde_json::from_reader(file)
            .map_err(|error| format!("invalid launch file {}: {error}", path.display()))?;
        if launch.version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported Canvas protocol version {}, expected {PROTOCOL_VERSION}",
                launch.version
            ));
        }
        Ok(launch)
    }
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Frame(IncomingFrame),
    Disconnected { channel: String },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct IncomingFrame {
    pub channel: String,
    pub frame_type: String,
    pub request_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireFrame {
    version: u8,
    id: String,
    widget_id: String,
    channel: String,
    #[serde(rename = "type")]
    frame_type: String,
    timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    payload: Value,
}

pub struct CanvasClient {
    launch: LaunchConfig,
    #[cfg(unix)]
    inner: std::sync::Arc<unix::Inner>,
}

impl CanvasClient {
    pub fn connect(launch: LaunchConfig, sender: Sender<ClientEvent>) -> Result<Self, String> {
        #[cfg(unix)]
        {
            let inner = unix::connect(&launch, sender)?;
            let client = Self {
                launch,
                inner: std::sync::Arc::new(inner),
            };
            client.send_hello("control")?;
            client.send_hello("event")?;
            Ok(client)
        }

        #[cfg(not(unix))]
        {
            let _ = (launch, sender);
            Err(
                "chart requires a Unix environment with tmux and Unix sockets; run it on Linux, macOS, or WSL"
                    .to_string(),
            )
        }
    }

    pub fn send_ready(&self, title: Option<&str>, request_id: Option<&str>) -> Result<(), String> {
        self.send_control(
            "ready",
            json!({
                "title": title,
                "capabilities": {
                    "state": true,
                    "selection": true,
                    "content": false,
                    "context": true,
                    "action": true,
                    "artifacts": true
                }
            }),
            request_id.map(str::to_string),
        )
    }

    pub fn send_state(&self, key: &str, data: Value) -> Result<(), String> {
        self.send_event("state", json!({ "key": key, "data": data }))
    }

    pub fn send_selection(&self, label: &str, text: String, data: Value) -> Result<(), String> {
        self.send_event(
            "selection",
            json!({ "label": label, "text": text, "data": data }),
        )
    }

    pub fn send_context(&self, label: &str, text: String, data: Value) -> Result<(), String> {
        self.send_event(
            "context",
            json!({ "label": label, "text": text, "data": data, "delivery": "context" }),
        )
    }

    pub fn send_action(&self, label: &str, prompt: String, data: Value) -> Result<(), String> {
        self.send_event(
            "action",
            json!({ "label": label, "prompt": prompt, "data": data, "delivery": "queue" }),
        )
    }

    pub fn send_artifact(&self, label: &str, path: &Path, text: String) -> Result<(), String> {
        self.send_event(
            "artifact",
            json!({
                "label": label,
                "path": path,
                "mediaType": "application/json",
                "text": text,
                "delivery": "context"
            }),
        )
    }

    pub fn request_close(&self) -> Result<(), String> {
        self.send_event("control", json!({ "command": "close" }))
    }

    pub fn send_pong(&self) -> Result<(), String> {
        self.send_control("pong", json!({}), None)
    }

    pub fn send_error(
        &self,
        message: &str,
        fatal: bool,
        request_id: Option<&str>,
    ) -> Result<(), String> {
        self.send_control(
            "error",
            json!({ "message": message, "fatal": fatal }),
            request_id.map(str::to_string),
        )
    }

    pub fn send_config_error(&self, message: &str, request_id: Option<&str>) -> Result<(), String> {
        self.send_error(message, false, request_id)
    }

    pub fn send_data_error(&self, message: &str, request_id: Option<&str>) -> Result<(), String> {
        self.send_error(message, false, request_id)
    }

    pub fn send_rpc_ok(&self, request_id: &str, data: Value) -> Result<(), String> {
        self.send_control(
            "rpc.response",
            json!({ "ok": true, "data": data }),
            Some(request_id.to_string()),
        )
    }

    pub fn send_rpc_error(&self, request_id: &str, error: &str) -> Result<(), String> {
        self.send_control(
            "rpc.response",
            json!({ "ok": false, "error": error }),
            Some(request_id.to_string()),
        )
    }

    pub fn close(&self) {
        #[cfg(unix)]
        self.inner.close();
    }

    fn send_hello(&self, channel: &str) -> Result<(), String> {
        let payload = json!({
            "token": self.launch.token,
            "kind": self.launch.kind,
            "scenario": self.launch.scenario,
            "pid": std::process::id()
        });
        if channel == "control" {
            self.send_control("hello", payload, None)
        } else {
            self.send_event("hello", payload)
        }
    }

    fn send_control(
        &self,
        frame_type: &str,
        payload: Value,
        request_id: Option<String>,
    ) -> Result<(), String> {
        self.send("control", frame_type, payload, request_id)
    }

    fn send_event(&self, frame_type: &str, payload: Value) -> Result<(), String> {
        self.send("event", frame_type, payload, None)
    }

    fn send(
        &self,
        channel: &str,
        frame_type: &str,
        payload: Value,
        request_id: Option<String>,
    ) -> Result<(), String> {
        let frame = WireFrame {
            version: PROTOCOL_VERSION,
            id: next_id(if channel == "control" { "ctl" } else { "evt" }),
            widget_id: self.launch.widget_id.clone(),
            channel: channel.to_string(),
            frame_type: frame_type.to_string(),
            timestamp: now_millis(),
            request_id,
            payload,
        };
        let mut encoded = serde_json::to_vec(&frame)
            .map_err(|error| format!("cannot encode Canvas frame: {error}"))?;
        encoded.push(b'\n');
        if encoded.len() > MAX_FRAME_BYTES {
            return Err(format!(
                "Canvas frame exceeds the {MAX_FRAME_BYTES} byte limit"
            ));
        }

        #[cfg(unix)]
        return self.inner.write(channel, &encoded);

        #[cfg(not(unix))]
        Err("Unix sockets are unavailable on this platform".to_string())
    }
}

fn validate_incoming(
    frame: WireFrame,
    expected_channel: &str,
    expected_widget: &str,
) -> Result<IncomingFrame, String> {
    if frame.version != PROTOCOL_VERSION {
        return Err(format!("unsupported frame version {}", frame.version));
    }
    if frame.channel != expected_channel {
        return Err(format!(
            "frame channel mismatch: expected {expected_channel}, got {}",
            frame.channel
        ));
    }
    if frame.widget_id != expected_widget {
        return Err("frame widgetId does not match launch widgetId".to_string());
    }
    if frame.id.is_empty() || frame.frame_type.is_empty() {
        return Err("frame id and type are required".to_string());
    }
    Ok(IncomingFrame {
        channel: frame.channel,
        frame_type: frame.frame_type,
        request_id: frame.request_id,
        payload: frame.payload,
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn next_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}_{}_{}_{}",
        std::process::id(),
        now_millis(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use super::*;

    pub struct Inner {
        control: Arc<Mutex<UnixStream>>,
        event: Arc<Mutex<UnixStream>>,
    }

    impl Inner {
        pub fn write(&self, channel: &str, bytes: &[u8]) -> Result<(), String> {
            let writer = if channel == "control" {
                &self.control
            } else {
                &self.event
            };
            let mut stream = writer
                .lock()
                .map_err(|_| format!("{channel} socket writer lock was poisoned"))?;
            stream
                .write_all(bytes)
                .map_err(|error| format!("cannot write {channel} socket: {error}"))?;
            stream
                .flush()
                .map_err(|error| format!("cannot flush {channel} socket: {error}"))
        }

        pub fn close(&self) {
            if let Ok(stream) = self.control.lock() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            if let Ok(stream) = self.event.lock() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn connect(launch: &LaunchConfig, sender: Sender<ClientEvent>) -> Result<Inner, String> {
        let control = connect_with_retry(&launch.control_socket_path, "control")?;
        let event = connect_with_retry(&launch.event_socket_path, "event")?;
        let control_reader = control
            .try_clone()
            .map_err(|error| format!("cannot clone control socket: {error}"))?;
        let event_reader = event
            .try_clone()
            .map_err(|error| format!("cannot clone event socket: {error}"))?;

        spawn_reader(
            control_reader,
            "control",
            launch.widget_id.clone(),
            sender.clone(),
        );
        spawn_reader(event_reader, "event", launch.widget_id.clone(), sender);

        Ok(Inner {
            control: Arc::new(Mutex::new(control)),
            event: Arc::new(Mutex::new(event)),
        })
    }

    fn connect_with_retry(path: &Path, channel: &str) -> Result<UnixStream, String> {
        let mut last_error = None;
        for attempt in 0..20 {
            match UnixStream::connect(path) {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis((50 * (attempt + 1)).min(500)));
                }
            }
        }
        Err(format!(
            "cannot connect {channel} socket {}: {}",
            path.display(),
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ))
    }

    fn spawn_reader(
        stream: UnixStream,
        channel: &'static str,
        widget_id: String,
        sender: Sender<ClientEvent>,
    ) {
        thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = sender.send(ClientEvent::Disconnected {
                            channel: channel.to_string(),
                        });
                        return;
                    }
                    Ok(_) if line.len() > MAX_FRAME_BYTES => {
                        let _ = sender.send(ClientEvent::Error(format!(
                            "host {channel} frame exceeded {MAX_FRAME_BYTES} bytes"
                        )));
                        return;
                    }
                    Ok(_) => {
                        let decoded = serde_json::from_str::<WireFrame>(line.trim_end())
                            .map_err(|error| format!("invalid host {channel} frame: {error}"))
                            .and_then(|frame| validate_incoming(frame, channel, &widget_id));
                        match decoded {
                            Ok(frame) => {
                                if sender.send(ClientEvent::Frame(frame)).is_err() {
                                    return;
                                }
                            }
                            Err(error) => {
                                let _ = sender.send(ClientEvent::Error(error));
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(ClientEvent::Error(format!(
                            "cannot read {channel} socket: {error}"
                        )));
                        return;
                    }
                }
            }
        });
    }
}
