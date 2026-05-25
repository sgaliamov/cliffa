use super::AppHandle;
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};
use std::{
    env,
    ffi::OsString,
    fs::File,
    path::{Path, PathBuf},
};
use tracing::{Level, debug, trace, warn};
use tracing_subscriber::{
    Layer,
    filter::FilterFn,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
};

/// Builds and runs a CLI application with tracing, config, and signal handling.
pub struct Builder {
    config_file: Option<PathBuf>,
    cli_aliases: FxHashMap<String, String>,
    env_prefix: Option<String>,
    level: Level,
    targets: Vec<(String, Level)>,
    with_level: bool,
    with_target: bool,
    with_thread_ids: bool,
    without_time: bool,
}

impl Default for Builder {
    /// Creates a builder with sensible defaults.
    fn default() -> Self {
        Self {
            config_file: None,
            cli_aliases: Default::default(),
            env_prefix: None,
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
    /// Sets the default tracing level.
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Sets per-target tracing levels.
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

    /// Configures whether thread IDs are shown in logs.
    pub fn with_thread_ids(mut self, value: bool) -> Self {
        self.with_thread_ids = value;
        self
    }

    /// Configures whether log levels are shown in logs.
    pub fn show_level(mut self, value: bool) -> Self {
        self.with_level = value;
        self
    }

    /// Configures whether log targets are shown in logs.
    pub fn with_target(mut self, value: bool) -> Self {
        self.with_target = value;
        self
    }

    /// Configures whether timestamps are shown in logs.
    pub fn with_time(mut self, value: bool) -> Self {
        self.without_time = !value;
        self
    }

    /// Sets the explicit config file path.
    pub fn config_file<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.config_file = Some(path.into());
        self
    }

    /// Sets the environment variable prefix used for config overrides.
    pub fn env_prefix<S: Into<String>>(mut self, prefix: S) -> Self {
        self.env_prefix = Some(prefix.into());
        self
    }

    /// Adds terminal input aliases that map to full config paths.
    pub fn with_cli_aliases<I, A, P>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = (A, P)>,
        A: Into<String>,
        P: Into<String>,
    {
        self.cli_aliases
            .extend(aliases.into_iter().map(|(alias, path)| {
                let alias = normalize_cli_alias(&alias.into());
                let path = normalize_cli_path(&path.into());
                (alias, path)
            }));
        self
    }

    /// Adds a single terminal input alias that maps to a full config path.
    pub fn with_cli_alias<A, P>(mut self, alias: A, path: P) -> Self
    where
        A: Into<String>,
        P: Into<String>,
    {
        let alias = normalize_cli_alias(&alias.into());
        let path = normalize_cli_path(&path.into());
        self.cli_aliases.insert(alias, path);
        self
    }

    /// Runs the application with the resolved configuration and app handle.
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

    /// Installs the Ctrl-C handler and returns a shared app handle.
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

    /// Sets up tracing subscriber output.
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

    /// Loads config from file, environment, and terminal input in precedence order.
    fn load_config<C: DeserializeOwned>(&self) -> Option<C> {
        let mut root = Value::Object(Map::new());

        if let Some(file_config) = self.load_file_config() {
            deep_merge(&mut root, file_config);
        }

        deep_merge(&mut root, env_to_json(self.env_prefix.as_deref()));

        deep_merge(
            &mut root,
            cli_args_to_json(env::args_os().skip(1), &self.cli_aliases),
        );

        if matches!(&root, Value::Object(map) if map.is_empty()) {
            return None;
        }

        match serde_json::from_value(root) {
            Ok(config) => Some(config),
            Err(error) => {
                warn!("Failed to deserialize merged config: {error}");
                None
            }
        }
    }

    /// Loads the base JSON config file.
    fn load_file_config(&self) -> Option<Value> {
        if let Some(ref file) = self.config_file {
            return load_json_value(file);
        }

        let mut current = env::current_exe().ok()?;
        current.set_extension("json");
        let file_name = current.file_name()?;

        // tbd: [cliffa] merge multiple configs from all paths on top.
        // tbd: [cliffa] use environment name to select correct config file.
        current
            .ancestors()
            .skip(1)
            .map(|dir| dir.join(file_name))
            .find_map(|path| load_json_value(&path))
    }
}

/// Loads a JSON document from disk.
fn load_json_value(path: &Path) -> Option<Value> {
    trace!("Looking for a config file from {}...", path.display());

    if !path.exists() {
        return None;
    }

    debug!("Loading a config file from {}...", path.display());

    match File::open(path) {
        Ok(file) => match serde_json::from_reader::<_, Value>(file) {
            Ok(value) => {
                trace!("Loaded config from {}", path.display());
                Some(value)
            }
            Err(error) => {
                warn!("Failed to parse config from {}: {error}", path.display());
                None
            }
        },
        Err(error) => {
            warn!("Failed to open config file {}: {error}", path.display());
            None
        }
    }
}

/// Converts matching environment variables into a JSON object.
fn env_to_json(prefix: Option<&str>) -> Value {
    let normalized_prefix = prefix.map(normalize_env_prefix);
    let mut root = Value::Object(Map::new());

    for (key, raw) in env::vars() {
        let Some(path) = env_key_to_path(&key, normalized_prefix.as_deref()) else {
            continue;
        };

        insert_path(&mut root, &path, parse_scalar(&raw));
    }

    root
}

/// Converts command-line overrides into a JSON object.
fn cli_args_to_json<I>(args: I, aliases: &FxHashMap<String, String>) -> Value
where
    I: IntoIterator<Item = OsString>,
{
    let mut root = Value::Object(Map::new());
    let mut pending_path: Option<String> = None;

    for arg in args {
        let arg = match arg.to_str() {
            Some(arg) => arg,
            None => {
                warn!("Skipping non-Unicode terminal input");
                continue;
            }
        };

        if let Some(flag) = parse_cli_flag(arg) {
            if let Some((path, raw)) = flag.split_once('=') {
                let path = resolve_cli_input_path(arg, path, aliases);
                insert_path(&mut root, &path, parse_scalar(raw));
                pending_path = None;
                continue;
            }

            let flag = resolve_cli_input_path(arg, flag, aliases);

            if let Some(previous) = pending_path.replace(flag) {
                insert_path(&mut root, &previous, Value::Bool(true));
            }

            continue;
        }

        if let Some(path) = pending_path.take() {
            insert_path(&mut root, &path, parse_scalar(arg));
        }
    }

    if let Some(path) = pending_path {
        insert_path(&mut root, &path, Value::Bool(true));
    }

    root
}

/// Normalizes the configured environment prefix.
fn normalize_env_prefix(prefix: &str) -> String {
    prefix.trim_end_matches('_').to_ascii_uppercase()
}

/// Maps an environment variable name to a config path.
fn env_key_to_path(key: &str, prefix: Option<&str>) -> Option<String> {
    let normalized_key = key.to_ascii_uppercase();

    let remainder = match prefix {
        Some(prefix) => normalized_key.strip_prefix(prefix)?.strip_prefix('_')?,
        None => key,
    };

    if remainder.is_empty() {
        return None;
    }

    Some(remainder.to_ascii_lowercase().replace("__", "."))
}

/// Parses a `--path.to.field` style command-line flag.
fn parse_cli_flag(value: &str) -> Option<&str> {
    if let Some(flag) = value.strip_prefix("--") {
        return (!flag.is_empty()).then_some(flag);
    }

    value
        .strip_prefix('-')
        .filter(|flag| !flag.is_empty() && !flag.starts_with('-'))
}

/// Normalizes a terminal input alias key for lookup.
fn normalize_cli_alias(alias: &str) -> String {
    alias.trim_start_matches('-').replace('.', "-")
}

/// Normalizes terminal input flag names into config paths.
fn normalize_cli_path(path: &str) -> String {
    if path.contains('.') {
        path.to_owned()
    } else {
        path.replace('-', ".")
    }
}

/// Resolves aliases before applying default CLI path normalization.
fn resolve_cli_path(path: &str, aliases: &FxHashMap<String, String>) -> String {
    let alias = normalize_cli_alias(path);

    aliases
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| normalize_cli_path(path))
}

/// Resolves terminal input path based on original flag style.
fn resolve_cli_input_path(arg: &str, path: &str, aliases: &FxHashMap<String, String>) -> String {
    if arg.starts_with("--") {
        return normalize_cli_path(path);
    }

    if aliases.contains_key(path) {
        return resolve_cli_path(path, aliases);
    }

    normalize_cli_path(path)
}

/// Inserts a value into a nested JSON object path.
fn insert_path(root: &mut Value, path: &str, value: Value) {
    if path.is_empty() {
        return;
    }

    let parts = path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return;
    }

    insert_path_parts(root, &parts, value);
}

/// Inserts a value into a nested JSON object path slice.
fn insert_path_parts(current: &mut Value, parts: &[&str], value: Value) {
    if parts.is_empty() {
        return;
    }

    if !matches!(current, Value::Object(_)) {
        *current = Value::Object(Map::new());
    }

    let Value::Object(map) = current else {
        return;
    };

    if parts.len() == 1 {
        map.insert(parts[0].to_owned(), value);
        return;
    }

    let next = map
        .entry(parts[0].to_owned())
        .or_insert_with(|| Value::Object(Map::new()));

    insert_path_parts(next, &parts[1..], value);
}

/// Deep-merges one JSON value onto another.
fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                if let Some(target_value) = target_map.get_mut(&key) {
                    deep_merge(target_value, source_value);
                } else {
                    target_map.insert(key, source_value);
                }
            }
        }
        (target_slot, source_value) => {
            *target_slot = source_value;
        }
    }
}

/// Parses a string into a JSON scalar when possible.
fn parse_scalar(raw: &str) -> Value {
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }

    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }

    if let Ok(number) = raw.parse::<i64>() {
        return Value::Number(Number::from(number));
    }

    if let Ok(number) = raw.parse::<u64>() {
        return Value::Number(Number::from(number));
    }

    if let Ok(number) = raw.parse::<f64>()
        && let Some(number) = Number::from_f64(number)
    {
        return Value::Number(number);
    }

    Value::String(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        cli_args_to_json, deep_merge, env_key_to_path, normalize_cli_alias, normalize_cli_path,
        parse_scalar, resolve_cli_input_path, resolve_cli_path,
    };
    use rustc_hash::FxHashMap;
    use serde_json::json;
    use std::ffi::OsString;

    #[test]
    fn cli_args_map_to_nested_json() {
        let value = cli_args_to_json(
            [
                OsString::from("--name"),
                OsString::from("cli-name"),
                OsString::from("--server.host=0.0.0.0"),
                OsString::from("--server.port"),
                OsString::from("9000"),
                OsString::from("--debug"),
            ],
            &FxHashMap::default(),
        );

        assert_eq!(
            value,
            json!({
                "name": "cli-name",
                "server": {
                    "host": "0.0.0.0",
                    "port": 9000,
                },
                "debug": true,
            })
        );
    }

    #[test]
    fn env_keys_map_to_nested_paths() {
        let path = env_key_to_path("APP_SERVER__PORT", Some("APP"));

        assert_eq!(path.as_deref(), Some("server.port"));
    }

    #[test]
    fn env_keys_without_prefix_map_to_nested_paths() {
        let path = env_key_to_path("server__port", None);

        assert_eq!(path.as_deref(), Some("server.port"));
    }

    #[test]
    fn env_keys_without_prefix_keep_single_underscores() {
        let path = env_key_to_path("rayon_num_threads", None);

        assert_eq!(path.as_deref(), Some("rayon_num_threads"));
    }

    #[test]
    fn later_sources_override_earlier_sources() {
        let mut base = json!({
            "name": "file",
            "server": {
                "host": "127.0.0.1",
                "port": 8080,
            }
        });
        let env = json!({
            "server": {
                "port": 9000,
            }
        });
        let cli = json!({
            "name": "cli",
        });

        deep_merge(&mut base, env);
        deep_merge(&mut base, cli);

        assert_eq!(
            base,
            json!({
                "name": "cli",
                "server": {
                    "host": "127.0.0.1",
                    "port": 9000,
                }
            })
        );
    }

    #[test]
    fn scalar_parser_handles_basic_types() {
        assert_eq!(parse_scalar("true"), json!(true));
        assert_eq!(parse_scalar("42"), json!(42));
        assert_eq!(parse_scalar("2.5"), json!(2.5));
        assert_eq!(parse_scalar("hello"), json!("hello"));
    }

    #[test]
    fn hyphenated_terminal_flags_map_to_nested_paths() {
        let value = cli_args_to_json(
            [
                OsString::from("--server-host"),
                OsString::from("0.0.0.0"),
                OsString::from("--server-port=9000"),
            ],
            &FxHashMap::default(),
        );

        assert_eq!(
            value,
            json!({
                "server": {
                    "host": "0.0.0.0",
                    "port": 9000,
                }
            })
        );
    }

    #[test]
    fn dotted_terminal_flags_stay_unchanged() {
        assert_eq!(normalize_cli_path("server.host"), "server.host");
    }

    #[test]
    fn aliases_map_short_flags_to_nested_paths() {
        let aliases = FxHashMap::from_iter([
            (String::from("host"), String::from("server.host")),
            (String::from("port"), String::from("server.port")),
        ]);
        let value = cli_args_to_json(
            [
                OsString::from("-host"),
                OsString::from("0.0.0.0"),
                OsString::from("-port=9000"),
            ],
            &aliases,
        );

        assert_eq!(
            value,
            json!({
                "server": {
                    "host": "0.0.0.0",
                    "port": 9000,
                }
            })
        );
    }

    #[test]
    fn alias_lookup_normalizes_dotted_keys() {
        let aliases =
            FxHashMap::from_iter([(String::from("server-host"), String::from("bind.host"))]);

        assert_eq!(normalize_cli_alias("server.host"), "server-host");
        assert_eq!(resolve_cli_path("server.host", &aliases), "bind.host");
    }

    #[test]
    fn single_dash_uses_aliases() {
        let aliases = FxHashMap::from_iter([(String::from("p"), String::from("server.port"))]);
        let value = cli_args_to_json([OsString::from("-p=9000")], &aliases);

        assert_eq!(value, json!({ "server": { "port": 9000 } }));
    }

    #[test]
    fn double_dash_uses_full_name() {
        let aliases = FxHashMap::from_iter([(String::from("port"), String::from("server.port"))]);

        assert_eq!(
            resolve_cli_input_path("-port", "port", &aliases),
            "server.port"
        );
        assert_eq!(resolve_cli_input_path("--port", "port", &aliases), "port");
        assert_eq!(normalize_cli_path("server-port"), "server.port");
        let value = cli_args_to_json([OsString::from("--server-port=9000")], &aliases);

        assert_eq!(value, json!({ "server": { "port": 9000 } }));
    }
}
