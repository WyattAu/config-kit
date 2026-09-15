//! Layered typed configuration for Rust.
//!
//! `config-kit` centralizes the estate's shared configuration pattern:
//! `serde`-typed settings assembled from layered sources — a base file,
//! then environment variables, then in-process overrides — with
//! unknown-key denial at the top level, redacted secrets, and optional
//! hot reload.
//!
//! # Design
//!
//! - **Typed and layered.** You describe the target type; layers produce
//!   values that deep-merge key-by-key in the order added. Driven in the
//!   documented order, precedence is **overrides > env > file**: later
//!   layers win on colliding keys, and table contents merge so a late
//!   layer can override one nested field without clobbering siblings.
//! - **Strict by choice.** [`ConfigBuilder::load`] ignores unknown keys;
//!   [`ConfigBuilder::load_strict`] denies unconsumed **top-level** keys
//!   with a typed error listing them — container-agnostically, for any
//!   `DeserializeOwned` type, without derive support (see
//!   [`load_strict`](ConfigBuilder::load_strict) for the exact contract).
//! - **Secrets are redacted by construction.** [`Sensitive<T>`] renders
//!   `"[REDACTED]"` for `Debug` and `Display` (bound-free impls, so
//!   derived container `Debug`s cannot leak either) and requires an
//!   explicit, grep-friendly `.expose()` to read.
//! - **Formats.** TOML is core. JSON (`json` feature) and YAML (`yaml`
//!   feature) are optional; format is picked by file extension, defaulting
//!   to TOML.
//! - **Hot reload, opt-in.** The `hot-reload` feature adds
//!   [`watch`]: a 300 ms-debounced, `notify`-backed watcher that feeds
//!   freshly parsed values to your callback and records reload failures
//!   instead of panicking.
//! - **Tier-A hygiene.** `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`,
//!   clippy `unwrap_used`/`expect_used`/`panic`/`indexing_slicing` denied —
//!   the public surface is fallible and typed, never panicking.
//!
//! # Example
//!
//! File layer omitted here so the example runs stand-alone; combine all
//! three layers for the documented precedence:
//!
//! ```
//! use config_kit::{ConfigBuilder, ConfigLayer, Sensitive};
//! use serde::Deserialize;
//! use std::collections::BTreeMap;
//!
//! #[derive(Deserialize)]
//! struct Service {
//!     port: u16,
//!     debug: bool,
//!     token: Sensitive<String>,
//! }
//!
//! std::env::set_var("CKIT_EXAMPLE_SERVICE_TOKEN", "hunter2");
//! std::env::set_var("CKIT_EXAMPLE_SERVICE_DEBUG", "true");
//! let mut over: BTreeMap<String, toml::Value> = BTreeMap::new();
//! over.insert("port".into(), toml::Value::Integer(9090));
//!
//! let service: Service = ConfigBuilder::new()
//!     .layer(ConfigLayer::env_prefix("CKIT_EXAMPLE_SERVICE_"))
//!     .layer(ConfigLayer::overrides(over))
//!     .load()?;
//!
//! assert_eq!(service.port, 9090); // overrides beat env
//! assert!(service.debug);
//! assert_eq!(service.token.expose(), "hunter2"); // the only way in
//! assert_eq!(format!("{:?}", service.token), "[REDACTED]");
//! # Ok::<(), config_kit::ConfigError>(())
//! ```
//!
//! # Strict loading
//!
//! [`ConfigBuilder::load_strict`] rejects typo'd keys at the top level:
//!
//! ```no_run
//! use config_kit::{ConfigBuilder, ConfigLayer};
//! use serde::Deserialize;
//! use tempfile::TempDir;
//!
//! #[derive(Deserialize, Debug)]
//! struct Db {
//!     url: String,
//! }
//! #[derive(Deserialize, Debug)]
//! struct Settings {
//!     db: Db,
//! }
//!
//! let dir = TempDir::new().expect("tempdir");
//! let path = dir.path().join("settings.toml");
//! std::fs::write(
//!     &path,
//!     "db = { url = \"postgres://localhost\" }\ntypo_key = 1\n",
//! )
//! .expect("write");
//!
//! let error = ConfigBuilder::new()
//!     .layer(ConfigLayer::file(&path))
//!     .load_strict::<Settings>()
//!     .expect_err("typo_key is not a field");
//! assert!(error.to_string().contains("typo_key"), "{error}");
//! ```
//!
//! # Environment typing
//!
//! Env values are inferred: `true`/`false` → bool, integer → `i64`, float
//! → `f64`, else verbatim string; keys are prefix-stripped and lowercased
//! (`APP_PORT` → `port`). See [`ConfigLayer::env_prefix`].
//!
//! # Hot reload
//!
//! With the `hot-reload` feature, `watch` spawns a 300 ms-debounced,
//! `notify`-backed watcher that parses the file on settle and hands the
//! fresh value to your callback; reload failures are recorded (never
//! panic) and retrievable via `ConfigWatcher::last_error`, and
//! `ConfigWatcher::stop` joins the worker. See the `watch` function's
//! documentation for a runnable example.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod builder;
mod env;
mod error;
mod layer;
mod sensitive;
#[cfg(feature = "hot-reload")]
mod watcher;

pub use builder::{ConfigBuilder, Empty, Ready};
pub use error::ConfigError;
pub use layer::ConfigLayer;
pub use sensitive::Sensitive;
#[cfg(feature = "hot-reload")]
pub use watcher::{watch, ConfigWatcher};

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_are_wired() {
        let builder: crate::ConfigBuilder<crate::Empty> = crate::ConfigBuilder::new();
        let ready = builder.layer(crate::ConfigLayer::env_prefix("CKIT_WIRING_"));
        let _ = std::mem::size_of::<crate::Ready>();
        drop(ready);
    }
}
