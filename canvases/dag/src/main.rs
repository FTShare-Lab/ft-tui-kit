mod actions;
mod app;
mod config;
mod data;
mod interaction;
mod layout;
mod render;
mod routing;
mod runtime;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ft_canvas_runtime::{CanvasClient, LaunchConfig};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use serde_json::Value;

fn main() {
    if let Err(error) = start() {
        eprintln!("dag: {error}");
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

    let result = runtime::run(
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
    Err("usage: dag --launch-file <launch.json>".to_string())
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open config file {}: {error}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|error| format!("invalid config file {}: {error}", path.display()))
}
