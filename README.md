# config-kit

Layered typed configuration for Rust — the shared config pattern of the
WyattAu estate: **file → env → overrides**, deep-merged into serde structs,
with top-level unknown-key denial, redacted secrets, and opt-in hot reload.

- **Typed, layered, deterministic**: a `ConfigBuilder` state machine that
  makes zero-layer loads a compile error; layers merge key-by-key in the
  order added, so later layers win on collisions without clobbering
  siblings.
- **Strict mode without derives**: `load_strict` denies unknown
  **top-level** keys for *any* `DeserializeOwned` type — no
  `#[serde(deny_unknown_fields)]` per-struct ceremony — and reports the
  offending keys in a typed error.
- **Secrets redacted by construction**: `Sensitive<T>` renders
  `"[REDACTED]"` for `Debug` *and* `Display`, with bound-free impls so
  derived container formatting cannot leak; reads require an explicit,
  grep-friendly `.expose()`.
- **TOML core; JSON/YAML optional**: format picked by file extension,
  TOML by default, `json`/`yaml` features add the rest.
- **Hot reload (opt-in)**: `notify`-backed, 300 ms debounced, typed
  callbacks; reload failures are recorded, never panics.
- **`#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`**, clippy
  `unwrap_used`/`expect_used`/`panic`/`indexing_slicing` denied — the
  public surface is fallible and typed.

## Install

```toml
[dependencies]
config-kit = "0.1"
```

## Example

```rust
use config_kit::{ConfigBuilder, ConfigLayer, Sensitive};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Service {
    port: u16,
    debug: bool,
    token: Sensitive<String>,
}

// Deployment env wins over the file; in-process overrides win over both.
std::env::set_var("SERVICE_TOKEN", "hunter2");
let mut over: BTreeMap<String, toml::Value> = BTreeMap::new();
over.insert("port".into(), toml::Value::Integer(9090));

let service: Service = ConfigBuilder::new()
    .layer(ConfigLayer::file("service.toml"))       // base
    .layer(ConfigLayer::env_prefix("SERVICE_"))     // deployment wins over file
    .layer(ConfigLayer::overrides(over))            // in-process wins over both
    .load()?;

assert_eq!(service.port, 9090);
assert_eq!(service.token.expose(), "hunter2"); // the only way in
```

Precedence: **overrides > env > file**. Tables deep-merge, so an override
of `db.port` leaves `db.url` from the file intact.

### Strict loading

```rust,ignore
let error = ConfigBuilder::new()
    .layer(ConfigLayer::file("service.toml"))
    .load_strict::<Service>()          // <- typo'd top-level keys rejected
    .expect_err("unknown keys");
// ConfigError::UnknownFields { keys: ["tokne"] }
```

The precise contract: detection is container-agnostic (it works for any
`DeserializeOwned` type by observing which top-level keys serde actually
consumed), it covers keys from *all* layers — file, env, and overrides —
and it is **top level only**: unknown keys nested inside tables are
serde's business, and `#[serde(flatten)]` fields are exempt (flattening
swallows the evidence before the check can see it).

### Hot reload

```toml
[dependencies]
config-kit = { version = "0.1", features = ["hot-reload"] }
```

```rust,ignore
let watcher = config_kit::watch::<Service, _>(&"service.toml".into(), move |service| {
    // freshly parsed after 300 ms of file quiet
    swap_active_config(service);
})?;
// Reload failures (deleted file, malformed edit) are recorded, not fatal:
if let Some(error) = watcher.last_error() { warn!("config reload: {error}") }
watcher.stop();
```

## Performance

Measured with criterion on the committed bench suite (`cargo bench`); see
`benches/loader_bench.rs` (3 layers × 20 keys each: file + env + overrides,
full merge-and-deserialize pipeline; Linux x86_64, rustc stable):

| Benchmark | Measured | Cost model |
|---|---|---|
| `layer_merge_3x20` | ~16 µs | 3 layer parses + deep merge + one serde pass |
| `env_parse_20` | ~11 µs | 20 `vars_os` entries, inference + table build + serde |

Loading is a cold-path operation: one file read + parse per file layer, one
`vars_os` sweep per env layer, and a single serde pass over the merged
table. `Sensitive<T>` adds zero runtime cost to non-formatting paths —
redaction is a constant string write. Publish your measured numbers in
your README per the estate standard.

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `json` | no | `.json` config files via `serde_json` |
| `yaml` | no | `.yaml`/`.yml` config files via `serde_yaml_ng` |
| `serde` | no | opt-in `Serialize` for `Sensitive<T>` (deserialization always available) |
| `hot-reload` | no | `watch(path, handler)` + `ConfigWatcher` via `notify` |

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.
