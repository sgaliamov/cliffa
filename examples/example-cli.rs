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
        .run(run)
}

// App logic.
fn run(_config: Option<Config>, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    while app.is_running() {
        debug!("I'm running!");
        sleep(Duration::from_secs(2));
    }

    Ok(())
}

// Configuration file and command line arguments.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(dead_code)]
struct Config {
    pub name: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub path: Option<PathBuf>,
}
