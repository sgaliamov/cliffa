# CLI

Mini CLI framework.

1. Parses the command line arguments.
1. Loads configuration from JSON files, environment variables, and terminal input.
1. Handles CTRL-C and termination signals.

Configuration precedence is fixed:

1. JSON file
1. Environment variables
1. Terminal input

Environment variables use a prefix and `__` for nested fields. Terminal input uses `--path.to.field value`, `--path.to.field=value`, or hyphenated forms like `--server-host value`.

CLI aliases can map short flags to full config paths. This is useful when nested field paths are too long or when you want stable public flag names.

Nested field mapping works like this:

- `APP_NAME=value` → `config.name`
- `APP_SERVER__HOST=value` → `config.server.host`
- `APP_SERVER__PORT=9000` → `config.server.port`
- `--name value` → `config.name`
- `--server.host value` → `config.server.host`
- `--server.port=9000` → `config.server.port`
- `--server-host value` → `config.server.host`
- `--server-port=9000` → `config.server.port`
- bare terminal flags like `--debug` are treated as `true`

```rust
use cliffa::cli::{self, AppHandle};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
	name: String,
	server: ServerConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerConfig {
	host: String,
	port: u16,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	cli::Builder::default()
		.env_prefix("APP")
		.with_cli_aliases([
			("host", "server.host"),
			("port", "server.port"),
		])
		.run(run)
}

fn run(config: Option<Config>, _app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
	dbg!(config);
	Ok(())
}
```

```powershell
$env:APP_SERVER__PORT=9000
cargo run --example example-cli -- --name cli-name --server.host 0.0.0.0
```

Hyphenated terminal flags work too:

```powershell
cargo run --example example-cli -- --server-host 0.0.0.0 --server-port=9000
```

Aliases can target nested fields too:

```powershell
cargo run --example example-cli -- --host 0.0.0.0 --port=9000
```

With the aliases above, that maps to:

- `--host` → `config.server.host`
- `--port` → `config.server.port`

Example input for a nested config:

```json
{
	"name": "from-file",
	"server": {
		"host": "127.0.0.1",
		"port": 8080
	}
}
```

```powershell
$env:APP_SERVER__PORT=9000
cargo run --example example-cli -- --name cli-name --server.host 0.0.0.0
```

Produces the final config values:

```text
name = "cli-name"
server.host = "0.0.0.0"
server.port = 9000
```

Reason:

- file sets the base values
- env overrides `server.port`
- terminal input overrides `name` and `server.host`

To run the sample application:

``` sh
cargo run --example main
```

## To do

- show progress lib.
