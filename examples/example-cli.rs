use chrono::{DateTime, Utc};
use cliffa::cli::{self, AppHandle};
use serde::Deserialize;
use std::{path::PathBuf, thread::sleep, time::Duration};
use tracing::{Level, debug};

// Entry point for the application.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::Builder::default()
        .with_level(Level::INFO)
        .with_targets([("example_cli", Level::DEBUG)])
        .env_prefix("EXAMPLE")
        .run(run)
}

// App logic.
fn run(config: Option<Config>, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(config) = config {
        debug!(
            name = config.name,
            start_time = ?config.start_time,
            end_time = ?config.end_time,
            path = ?config.path,
            "Loaded layered config"
        );
    }

    while app.is_running() {
        debug!("I'm running!");
        sleep(Duration::from_secs(2));
    }

    Ok(())
}

// Configuration file, environment variables, and terminal input.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    pub name: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub path: Option<PathBuf>,
}
