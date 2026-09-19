//! Errors produced while loading, validating, and watching configuration.

use std::path::PathBuf;

/// Loading, parsing, strict-validation, and watching failures.
///
/// Documented failure modes are part of the API contract; every variant
/// lists the condition that produces it. The enum is `#[non_exhaustive]`,
/// so matches require a wildcard arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Reading a configuration file failed at the OS level — the file does
    /// not exist, lacks read permission, or the path is a directory.
    /// Produced by [`file`](crate::ConfigLayer::file) layers and by
    /// [`watch`](crate::watch) when the watched file cannot be read back.
    #[error("failed to read config file {}: {source}", path.display())]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// One of:
    ///
    /// - a configuration file failed to parse in its format,
    /// - the file's extension names a format whose feature is disabled
    ///   (`json`, `yaml`),
    /// - the merged configuration did not deserialize into the target type.
    ///
    /// `path` names where the failure happened. For a file parse failure it
    /// is the file involved. For a merge/deserialization mismatch it is the
    /// origin of the offending **value**: the file that supplied it,
    /// `<env:VARIABLE>` for a value supplied by an environment layer (the
    /// full, pre-strip variable name), or `<overrides>` for the in-process
    /// override layer — so a deployment's bad environment variable is
    /// reported as itself, not as the file it happened to override. The
    /// path is empty when nothing more specific applies (no file layer
    /// contributed and the failing value's layer could not be determined);
    /// Display renders the empty path as `(merged)`.
    #[error("failed to parse config file {}: {source}", Self::parse_path(path))]
    Parse {
        /// The file involved, or empty for a non-file deserialization
        /// mismatch (see above).
        path: PathBuf,
        /// The parser or serde error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// [`load_strict`](crate::ConfigBuilder::load_strict) found top-level
    /// keys that no field of the target type consumed. Nested unknown keys
    /// are deliberately not reported (see `load_strict`'s documentation for
    /// the precise contract).
    #[error("unknown top-level configuration keys: {}", keys.join(", "))]
    UnknownFields {
        /// The unconsumed top-level keys, sorted and deduplicated.
        keys: Vec<String>,
    },
    /// An environment variable matching the layer prefix carries a value
    /// that is not valid Unicode. Variables whose *key* is not valid
    /// Unicode can never match a `str` prefix and are skipped instead.
    #[error("environment variable `{key}` is not valid Unicode: {source}")]
    EnvVar {
        /// The (valid-Unicode) variable name.
        key: String,
        /// The underlying variable error.
        #[source]
        source: std::env::VarError,
    },
    /// File watching could not start — the platform watcher could not be
    /// created, the path could not be registered (typically: it does not
    /// exist), or the watcher thread could not be spawned. Produced only by
    /// [`watch`](crate::watch) (feature `hot-reload`).
    #[error("failed to start file watcher: {source}")]
    WatchFailed {
        /// The underlying watcher error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The watcher's shared error-slot lock was poisoned by a panic in the
    /// watcher thread. Treat as fatal: reload state is indeterminate and
    /// the watcher must be stopped and rebuilt.
    #[error("config watcher lock poisoned; watcher state is unusable")]
    Poisoned,
}

impl ConfigError {
    /// Renders `Parse`'s path, collapsing the empty path to `(merged)`.
    fn parse_path(path: &std::path::Path) -> std::borrow::Cow<'_, str> {
        if path.as_os_str().is_empty() {
            std::borrow::Cow::Borrowed("(merged)")
        } else {
            path.to_string_lossy()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn display_is_informative() {
        let io = ConfigError::Io {
            path: "/etc/app.toml".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        assert!(io.to_string().contains("/etc/app.toml"));
        assert!(io.to_string().contains("missing"));

        let parse = ConfigError::Parse {
            path: "cfg.toml".into(),
            source: "expected `=`".into(),
        };
        assert_eq!(
            parse.to_string(),
            "failed to parse config file cfg.toml: expected `=`"
        );

        let merged = ConfigError::Parse {
            path: PathBuf::new(),
            source: "invalid type".into(),
        };
        assert!(merged.to_string().contains("(merged)"));
        assert!(merged.to_string().contains("invalid type"));

        let unknown = ConfigError::UnknownFields {
            keys: vec!["typo_a".into(), "typo_b".into()],
        };
        assert_eq!(
            unknown.to_string(),
            "unknown top-level configuration keys: typo_a, typo_b"
        );

        let env = ConfigError::EnvVar {
            key: "APP_TOKEN".into(),
            source: std::env::VarError::NotPresent,
        };
        assert!(env.to_string().contains("APP_TOKEN"));

        let watch = ConfigError::WatchFailed {
            source: "inotify limit reached".into(),
        };
        assert!(watch.to_string().contains("inotify limit reached"));

        assert_eq!(
            ConfigError::Poisoned.to_string(),
            "config watcher lock poisoned; watcher state is unusable"
        );
    }

    #[test]
    fn errors_are_std_errors() {
        let boxed: Box<dyn std::error::Error> = Box::new(ConfigError::Poisoned);
        assert!(!boxed.to_string().is_empty());
    }
}
