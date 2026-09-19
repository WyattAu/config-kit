# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [0.1.1] - 2026-09-19

### Fixed

- **Parse errors now name the layer that supplied the offending value.**
  When an env-provided value failed to type-check, the
  `ConfigError::Parse` error cited the last **file** layer read — e.g. a
  deployment setting `APP_PORT=not-a-number` was reported as a parse
  failure of `app.toml`, sending operators to debug the wrong artifact.
  The layered merge now tracks per-value provenance (which layer last
  wrote each merged value, at its dotted path), and deserialization
  mismatches are attributed via the failing value's exact location:
  - a value from an environment layer is reported as `<env:VARIABLE>`
    (the full, pre-strip variable name),
  - a value from the overrides layer as `<overrides>`,
  - a value from a file layer as that file's path — now the file that
    actually supplied the bad value, not merely the most recently read
    one,
  - falling back to the previous behavior (last file layer, `(merged)`
    when none) when the failing location cannot be resolved.
  Strict (`load_strict`) loads attribute identically. Message format is
  unchanged (`failed to parse config file {location}: {source}`) — only
  the location is more precise.

### Added

- `serde_path_to_error` dependency (internal attribution plumbing; no
  public API change).

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
