//! Configuration sources and the precedence they compose with.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

/// One named source of configuration values.
///
/// Layers are applied by [`ConfigBuilder::layer`](crate::ConfigBuilder::layer)
/// in the order they are added. A later layer's keys replace earlier ones,
/// while tables merge key-by-key (deep merge), so a late layer can override
/// a single nested field without clobbering its siblings. Driven in the
/// documented order this yields precedence **overrides > env > file**.
#[derive(Debug, Clone)]
pub enum ConfigLayer {
    /// A configuration file, parsed at load time by extension: `.json`
    /// (feature `json`), `.yaml`/`.yml` (feature `yaml`), and anything
    /// else — including no extension — as TOML.
    File(PathBuf),
    /// All environment variables sharing a prefix. The prefix is stripped
    /// and the remainder lowercased to form the top-level key; values are
    /// typed by inference (see the crate root for the exact rules).
    EnvPrefix(String),
    /// In-process values, typically assembled by a supervisor or test.
    /// Applied last in the documented order, hence highest precedence.
    Overrides(BTreeMap<String, toml::Value>),
}

impl ConfigLayer {
    /// A file-backed layer parsed by extension (TOML core; JSON and YAML
    /// behind the matching features).
    #[must_use]
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self::File(path.as_ref().to_path_buf())
    }

    /// An environment layer matching every variable that starts with
    /// `prefix` (case-sensitive).
    #[must_use]
    pub fn env_prefix(prefix: impl Into<String>) -> Self {
        Self::EnvPrefix(prefix.into())
    }

    /// An in-process override layer keyed by top-level field name.
    #[must_use]
    pub fn overrides(overrides: BTreeMap<String, toml::Value>) -> Self {
        Self::Overrides(overrides)
    }
}
