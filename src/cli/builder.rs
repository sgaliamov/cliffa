use super::AppHandle;
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use std::{fs::File, path::PathBuf};
use tracing::{Level, debug, trace, warn};
use tracing_subscriber::{
    Layer,
    filter::FilterFn,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
};

pub struct Builder {
    config_file: Option<PathBuf>,
    level: Level,
    targets: Vec<(String, Level)>,
    with_level: bool,
    with_target: bool,
    with_thread_ids: bool,
    without_time: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            config_file: None,
            level: Level::INFO,
            targets: Default::default(),
            with_level: true,
            with_target: true,
            with_thread_ids: false,
            without_time: false,
        }
    }
}

impl Builder {
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    pub fn with_targets<I, S>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = (S, Level)>,
        S: Into<String>,
    {
        self.targets = targets
            .into_iter()
            .map(|(t, lvl)| (t.into(), lvl))
            .collect();
        self
    }

    pub fn with_thread_ids(mut self, value: bool) -> Self {
        self.with_thread_ids = value;
        self
    }

    pub fn show_level(mut self, value: bool) -> Self {
        self.with_level = value;
        self
    }

    pub fn with_target(mut self, value: bool) -> Self {
        self.with_target = value;
        self
    }

    pub fn with_time(mut self, value: bool) -> Self {
        self.without_time = !value;
        self
    }

    pub fn config_file<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.config_file = Some(path.into());
        self
    }

    pub fn run<Config, F, R, E>(self, application: F) -> Result<R, E>
    where
        Config: DeserializeOwned,
        F: FnOnce(Option<Config>, AppHandle) -> Result<R, E>,
    {
        // tbd: [cliffa] maybe return error instead of panic.
        self.setup_logging().expect("Failed to set up logging");

        let handle = self
            .setup_handle()
            .expect("Failed to set up Ctrl-C handler");

        let config = self.load_config::<Config>();

        application(config, handle)
    }

    fn setup_handle(&self) -> Result<AppHandle, ctrlc::Error> {
        let handle = AppHandle::new();
        let clone = handle.clone();

        ctrlc::set_handler(move || {
            if clone.is_running() {
                warn!("Aborting…");
                clone.finish();
            }
        })?;

        Ok(handle)
    }

    fn setup_logging(&self) -> Result<(), TryInitError> {
        let level = self.level;
        let map: FxHashMap<_, _> = self.targets.iter().cloned().collect();
        let filter = FilterFn::new(move |metadata| {
            // tbd: [cliffa] filter logs by mask.
            let max = map.get(metadata.target()).unwrap_or(&level);
            metadata.level() <= max
        });

        let layer = tracing_subscriber::fmt::layer()
            .with_level(self.with_level)
            .with_thread_ids(self.with_thread_ids)
            .with_target(self.with_target);

        // tbd: [cliffa] refactor ugliness
        if self.without_time {
            let layer = layer.without_time().with_filter(filter);
            tracing_subscriber::registry().with(layer).try_init()
        } else {
            let layer = layer.with_filter(filter);
            // tbd: [cliffa] setup short timer format.
            tracing_subscriber::registry().with(layer).try_init()
        }
    }

    /// Loads configuration file using the current name of the application.
    /// Looks in all folders up in the hierarchy.
    fn load_config<C: DeserializeOwned>(&self) -> Option<C> {
        if let Some(ref file) = self.config_file {
            return load_json(file);
        }

        let mut current = std::env::current_exe().ok()?;
        current.set_extension("json");
        let file_name = current.file_name()?;

        // tbd: [cliffa] merge multiple configs from all paths on top.
        // tbd: [cliffa] merge with environment variables.
        // tbd: [cliffa] merge with command line arguments.
        // tbd: [cliffa] use environment name to select correct config file.
        current
            .ancestors()
            .skip(1) // skip self
            .map(|dir| dir.join(file_name))
            .find_map(|path| load_json(&path))
    }
}

fn load_json<C: DeserializeOwned>(path: &PathBuf) -> Option<C> {
    trace!("Looking for a config file from {}...", path.display());

    if !path.exists() {
        return None;
    }

    debug!("Loading a config file from {}...", path.display());

    File::open(path)
        .ok()
        // tbd: [cliffa] handle error
        .and_then(|f| serde_json::from_reader(f).unwrap())
        .inspect(|_| {
            trace!("Loaded config from {}", path.display());
        })
}
