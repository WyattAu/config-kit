# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [0.1.0] - 2026-09-15

### Added

- `ConfigBuilder<S>` typed state machine: `new()` → `layer()` (file,
  `env_prefix`, overrides) → `load`/`load_strict`; zero-layer loads are a
  compile-time error. Deep-merge layering with documented precedence
  **overrides > env > file**.
- `load_strict`: container-agnostic denial of unknown **top-level** keys
  for any `DeserializeOwned` type via serde deserialization
  instrumentation (`serde_ignored`), returning
  `ConfigError::UnknownFields { keys }`.
- `Sensitive<T>` secret redaction: bound-free `Debug`/`Display` impls
  render `"[REDACTED]"`; explicit `.expose()`/`.into_inner()` reads;
  deserialization always available, serialization opt-in behind the
  `serde` feature.
- Typed env layers: prefix-stripped, lowercased keys with value type
  inference (bool/int/float/string).
- `hot-reload` feature: `watch(path, handler)` — `notify`-backed, 300 ms
  debounce, `Box<dyn Fn(T) + Send + Sync>` callback, reload failures
  recorded (never panicking) and retrievable via
  `ConfigWatcher::last_error`; `stop()` joins the worker.
- `ConfigError`: `Io`, `Parse`, `UnknownFields`, `EnvVar`, `WatchFailed`,
  `Poisoned` — `#[non_exhaustive]`, every variant's producer documented.
- Criterion benches (`layer_merge_3x20`, `env_parse_20`), layering /
  strictness / redaction / env-typing / error-variant integration tests,
  hot-reload callback tests behind the feature.
