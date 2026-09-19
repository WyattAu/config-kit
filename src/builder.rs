//! The typed builder and the layered load pipeline.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::Path;
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::error::ConfigError;
use crate::layer::ConfigLayer;

/// Builder state: no layers added yet.
///
/// [`load`](ConfigBuilder::load) is deliberately not callable in this
/// state — a load with zero layers is a compile-time type error, not a
/// runtime failure.
#[derive(Debug, Clone, Copy)]
pub struct Empty;

/// Builder state: at least one layer has been added, so the layer stack is
/// meaningful and loading is available.
#[derive(Debug, Clone, Copy)]
pub struct Ready;

/// Builder for layered, typed configuration.
///
/// The state parameter makes loading impossible until a layer exists:
/// [`ConfigBuilder::new`] returns [`ConfigBuilder<Empty>`](ConfigBuilder),
/// every [`layer`](Self::layer) call returns
/// [`ConfigBuilder<Ready>`](ConfigBuilder), and only the ready state
/// exposes [`load`](Self::load) / [`load_strict`](Self::load_strict).
///
/// # Precedence
///
/// Layers apply in the order added; later layers win on colliding keys,
/// with tables merging key-by-key (deep merge). Driven in the documented
/// order this gives **overrides > env > file**:
///
/// ```text
/// ConfigBuilder::new()
///     .layer(ConfigLayer::file("app.toml"))   // base
///     .layer(ConfigLayer::env_prefix("APP_")) // deployment env wins over file
///     .layer(ConfigLayer::overrides(over))    // in-process wins over both
///     .load::<AppSettings>()?;
/// ```
///
/// # Example
///
/// ```
/// use config_kit::{ConfigBuilder, ConfigLayer};
/// use serde::Deserialize;
/// use std::collections::BTreeMap;
///
/// #[derive(Deserialize)]
/// struct Settings {
///     port: u16,
///     debug: bool,
/// }
///
/// std::env::set_var("CKIT_DOC_PORT", "8080");
/// std::env::set_var("CKIT_DOC_DEBUG", "true");
/// let mut over: BTreeMap<String, toml::Value> = BTreeMap::new();
/// over.insert("port".into(), toml::Value::Integer(9090));
///
/// let settings: Settings = ConfigBuilder::new()
///     .layer(ConfigLayer::env_prefix("CKIT_DOC_"))
///     .layer(ConfigLayer::overrides(over))
///     .load()?;
///
/// assert_eq!(settings.port, 9090); // overrides beat env
/// assert!(settings.debug); // env value inferred as bool
/// # Ok::<(), config_kit::ConfigError>(())
/// ```
#[derive(Debug, Clone)]
pub struct ConfigBuilder<S> {
    layers: Vec<ConfigLayer>,
    _state: PhantomData<S>,
}

impl Default for ConfigBuilder<Empty> {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigBuilder<Empty> {
    /// A builder with no layers. Add at least one
    /// [`ConfigLayer`] to unlock loading.
    #[must_use]
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            _state: PhantomData,
        }
    }
}

impl<S> ConfigBuilder<S> {
    /// Adds a layer and transitions the builder to the [`Ready`] state.
    ///
    /// Layers take effect in the order added; see the type-level
    /// documentation for precedence and merge semantics.
    #[must_use]
    pub fn layer(mut self, layer: ConfigLayer) -> ConfigBuilder<Ready> {
        self.layers.push(layer);
        ConfigBuilder {
            layers: self.layers,
            _state: PhantomData,
        }
    }
}

/// Where a merged value came from — the attribution recorded as layers
/// merge, so a parse error can name the layer that supplied the offending
/// value.
#[derive(Debug, Clone)]
enum Origin {
    /// A file layer, identified by its path.
    File(PathBuf),
    /// An environment layer, identified by the full variable name
    /// (pre-strip, e.g. `APP_PORT`).
    Env { var: String },
    /// The in-process overrides layer.
    Overrides,
}

impl Origin {
    /// The location rendered into `ConfigError::Parse::path`: the file
    /// path itself, an `<env:VARIABLE>` pseudo-path, or `<overrides>`.
    fn error_path(&self) -> PathBuf {
        match self {
            Origin::File(path) => path.clone(),
            Origin::Env { var } => PathBuf::from(format!("<env:{var}>")),
            Origin::Overrides => PathBuf::from("<overrides>"),
        }
    }
}

/// The fully merged configuration plus per-value provenance for error
/// reporting.
struct Merged {
    value: toml::Value,
    /// Dotted path of every value a layer inserted or overwrote → the
    /// layer that last wrote it. Deep merges record at the leaf /
    /// replacement site, so attribution follows the winning layer.
    provenance: BTreeMap<String, Origin>,
    last_file: Option<PathBuf>,
}

impl ConfigBuilder<Ready> {
    /// Deserializes the merged configuration into `T`.
    ///
    /// Unknown keys are ignored (use [`load_strict`](Self::load_strict) to
    /// deny them); types must line up with `T`'s fields.
    ///
    /// On a type mismatch the error names the **origin of the offending
    /// value** — the file that supplied it, `<env:VARIABLE>` for an
    /// environment-supplied value, or `<overrides>` — not merely the last
    /// file layer read.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Io`] when a file layer cannot be read,
    /// - [`ConfigError::Parse`] when a file layer fails to parse, names a
    ///   format whose feature is disabled, or when the merged values do
    ///   not fit `T` (attributed to the offending value's layer, see
    ///   above),
    /// - [`ConfigError::EnvVar`] when a matching environment variable has
    ///   a non-Unicode value.
    pub fn load<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        let Merged {
            value,
            provenance,
            last_file,
        } = self.merged()?;
        match serde_path_to_error::deserialize(value) {
            Ok(value) => Ok(value),
            Err(source) => {
                let path = attribute_error_path(&provenance, &last_file, source.path());
                Err(ConfigError::Parse {
                    path,
                    source: Box::new(source),
                })
            }
        }
    }

    /// Strict load: fails with [`ConfigError::UnknownFields`] when any
    /// **top-level** key of the merged configuration is not consumed by a
    /// field of `T`.
    ///
    /// The precise, honest contract:
    ///
    /// - Detection is container-agnostic — it works for any `T:
    ///   DeserializeOwned` by instrumenting deserialization with
    ///   [`serde_ignored`] and reporting which top-level map keys no field
    ///   visited. It is *not* `#[serde(deny_unknown_fields)]` semantics
    ///   (that derive is per-struct; config-kit never sees your schema
    ///   declaration sites).
    /// - **Top level only.** Unknown keys nested inside tables (`[db]` →
    ///   `rogue = 1`) are passed through to serde's normal handling and
    ///   are not reported. Keys introduced by env or override layers are
    ///   checked identically to file keys.
    /// - Types using `#[serde(flatten)]` collect unknown keys into the
    ///   flattened field before instrumentation can see them, so flattened
    ///   content is exempt from this check.
    /// - If `T` itself derives `deny_unknown_fields`, its own error wins
    ///   and surfaces as [`ConfigError::Parse`].
    ///
    /// Like [`load`](Self::load), a deserialization mismatch is attributed
    /// to the offending value's layer (file, `<env:VARIABLE>`, or
    /// `<overrides>`).
    ///
    /// On success the deserialized value is returned directly — there is
    /// no second parse pass.
    ///
    /// # Errors
    ///
    /// Everything [`load`](Self::load) reports, plus
    /// [`ConfigError::UnknownFields`] when unconsumed top-level keys exist.
    pub fn load_strict<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        let Merged {
            value,
            provenance,
            last_file,
        } = self.merged()?;
        let mut unknown: Vec<String> = Vec::new();
        let mut track = serde_path_to_error::Track::new();
        let parsed = serde_ignored::deserialize(
            serde_path_to_error::Deserializer::new(value, &mut track),
            |path| {
                if let serde_ignored::Path::Map { parent, key } = path {
                    if matches!(*parent, serde_ignored::Path::Root) {
                        unknown.push(key);
                    }
                }
            },
        );
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(source) => {
                let path = attribute_error_path(&provenance, &last_file, &track.path());
                return Err(ConfigError::Parse {
                    path,
                    source: Box::new(source),
                });
            }
        };
        if unknown.is_empty() {
            return Ok(parsed);
        }
        unknown.sort_unstable();
        unknown.dedup();
        Err(ConfigError::UnknownFields { keys: unknown })
    }

    /// Walks the layers in order, merging each into the accumulator.
    ///
    /// Every inserted or replaced value is recorded in `provenance` with
    /// the layer that wrote it, so parse errors attribute to the winning
    /// layer rather than to "wherever we happened to be reading last".
    fn merged(self) -> Result<Merged, ConfigError> {
        let mut acc = toml::Value::Table(toml::Table::new());
        let mut provenance = BTreeMap::new();
        let mut last_file: Option<PathBuf> = None;
        for layer in self.layers {
            match layer {
                ConfigLayer::File(path) => {
                    let value = read_layer_value(&path)?;
                    last_file = Some(path.clone());
                    let origin = Origin::File(path);
                    let mut from_root = Vec::new();
                    merge_tracked(&mut acc, value, &origin, &mut from_root, &mut provenance);
                }
                ConfigLayer::EnvPrefix(prefix) => {
                    let (table, sources) = crate::env::collect(&prefix)?;
                    for (key, value) in table {
                        let var = sources.get(&key).cloned().unwrap_or_default();
                        let origin = Origin::Env { var };
                        let mut wrapper = toml::Table::new();
                        wrapper.insert(key, value);
                        let mut from_root = Vec::new();
                        merge_tracked(
                            &mut acc,
                            toml::Value::Table(wrapper),
                            &origin,
                            &mut from_root,
                            &mut provenance,
                        );
                    }
                }
                ConfigLayer::Overrides(overrides) => {
                    let mut table = toml::Table::new();
                    for (key, value) in overrides {
                        table.insert(key, value);
                    }
                    let origin = Origin::Overrides;
                    let mut from_root = Vec::new();
                    merge_tracked(
                        &mut acc,
                        toml::Value::Table(table),
                        &origin,
                        &mut from_root,
                        &mut provenance,
                    );
                }
            }
        }
        Ok(Merged {
            value: acc,
            provenance,
            last_file,
        })
    }
}

/// The path attributed to a deserialization mismatch at `failing`: the
/// origin of the value at (or nearest above) the failing key path, falling
/// back to the most recently read file layer, or empty when no file layer
/// contributed. Free-standing over `Merged`'s fields because `value` is
/// consumed by the deserializer before the error exists.
fn attribute_error_path(
    provenance: &BTreeMap<String, Origin>,
    last_file: &Option<PathBuf>,
    failing: &serde_path_to_error::Path,
) -> PathBuf {
    let segments: Vec<String> = failing
        .iter()
        .map(|segment| match segment {
            serde_path_to_error::Segment::Map { key } => key.clone(),
            serde_path_to_error::Segment::Enum { variant } => variant.clone(),
            serde_path_to_error::Segment::Seq { index } => index.to_string(),
            serde_path_to_error::Segment::Unknown => String::from("?"),
        })
        .collect();
    for end in (1..=segments.len()).rev() {
        let Some(prefix) = segments.get(..end) else {
            continue;
        };
        if let Some(origin) = provenance.get(&prefix.join(".")) {
            return origin.error_path();
        }
    }
    last_file.clone().unwrap_or_default()
}

/// Reads and parses one file layer.
///
/// Used by the builder and by the hot-reload watcher, so both paths share
/// one format-detection contract.
pub(crate) fn read_layer_value(path: &Path) -> Result<toml::Value, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_by_extension(path, &text)
}

/// Parses `text` as the format named by `path`'s extension: `.json` and
/// `.yaml`/`.yml` behind their features, everything else as TOML.
fn parse_by_extension(path: &Path, text: &str) -> Result<toml::Value, ConfigError> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("toml");
    match ext.to_ascii_lowercase().as_str() {
        "json" => parse_json(path, text),
        "yaml" | "yml" => parse_yaml(path, text),
        _ => parse_toml(path, text),
    }
}

/// The `Parse` error for a file that failed to parse.
fn parse_error(
    path: &Path,
    source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> ConfigError {
    ConfigError::Parse {
        path: path.to_path_buf(),
        source: source.into(),
    }
}

fn parse_toml(path: &Path, text: &str) -> Result<toml::Value, ConfigError> {
    toml::from_str(text).map_err(|source| parse_error(path, source))
}

#[cfg(feature = "json")]
fn parse_json(path: &Path, text: &str) -> Result<toml::Value, ConfigError> {
    serde_json::from_str(text).map_err(|source| parse_error(path, source))
}

#[cfg(not(feature = "json"))]
fn parse_json(path: &Path, _text: &str) -> Result<toml::Value, ConfigError> {
    Err(parse_error(
        path,
        "`json` config files require the `json` feature",
    ))
}

#[cfg(feature = "yaml")]
fn parse_yaml(path: &Path, text: &str) -> Result<toml::Value, ConfigError> {
    serde_yaml_ng::from_str(text).map_err(|source| parse_error(path, source))
}

#[cfg(not(feature = "yaml"))]
fn parse_yaml(path: &Path, _text: &str) -> Result<toml::Value, ConfigError> {
    Err(parse_error(
        path,
        "`yaml` config files require the `yaml` feature",
    ))
}

/// Deep-merges `over` into `base`: two tables merge key-by-key, anything
/// else (including a table over a non-table) replaces outright.
///
/// Every insert or replacement is recorded in `provenance` under its
/// dotted path (relative to the merge root, tracked via `from_root`) with
/// the `origin` that performed it — last writer wins, matching which value
/// actually survives the merge.
fn merge_tracked(
    base: &mut toml::Value,
    over: toml::Value,
    origin: &Origin,
    from_root: &mut Vec<String>,
    provenance: &mut BTreeMap<String, Origin>,
) {
    match (base, over) {
        (toml::Value::Table(base_table), toml::Value::Table(over_table)) => {
            for (key, value) in over_table {
                match base_table.get_mut(&key) {
                    Some(slot) => {
                        from_root.push(key);
                        merge_tracked(slot, value, origin, from_root, provenance);
                        from_root.pop();
                    }
                    None => {
                        from_root.push(key.clone());
                        record_provenance(from_root, origin, provenance);
                        base_table.insert(key, value);
                        from_root.pop();
                    }
                }
            }
        }
        (base, over) => {
            if !from_root.is_empty() {
                record_provenance(from_root, origin, provenance);
            }
            *base = over;
        }
    }
}

/// Attributes the value at `path` to `origin` — the last writer wins,
/// because that writer's value is the one that survived.
fn record_provenance(path: &[String], origin: &Origin, provenance: &mut BTreeMap<String, Origin>) {
    provenance.insert(path.join("."), origin.clone());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn merge_tables_deep_and_replace_scalars() {
        let mut base: toml::Value =
            toml::from_str("flag = [1, 2]\n\n[db]\nurl = \"u\"\nport = 1\n").expect("base");
        let over: toml::Value = toml::from_str("flag = 3\n\n[db]\nport = 2\n").expect("over");
        let origin = Origin::File(PathBuf::from("over.toml"));
        let mut provenance = BTreeMap::new();
        merge_tracked(&mut base, over, &origin, &mut Vec::new(), &mut provenance);
        let table = base.as_table().expect("table");
        let db = table.get("db").and_then(toml::Value::as_table).expect("db");
        assert_eq!(db.get("url"), Some(&toml::Value::String("u".into())));
        assert_eq!(db.get("port"), Some(&toml::Value::Integer(2)));
        assert_eq!(table.get("flag"), Some(&toml::Value::Integer(3)));
        // Provenance: last writer wins per replaced value, untouched
        // values keep their earlier origin (none here, so only the two
        // writes are recorded — keyed by dotted path).
        assert!(matches!(provenance.get("flag"), Some(Origin::File(_))));
        assert!(matches!(provenance.get("db.port"), Some(Origin::File(_))));
        assert!(
            !provenance.contains_key("db.url"),
            "unwritten values have no provenance"
        );
        assert!(
            !provenance.contains_key("db"),
            "table recursions don't record the parent"
        );
    }

    #[test]
    fn parse_by_extension_detects_formats() {
        let toml_val = parse_by_extension(Path::new("a.toml"), "x = 1").expect("toml");
        assert_eq!(toml_val.get("x"), Some(&toml::Value::Integer(1)));
        let no_ext = parse_by_extension(Path::new("defaults"), "x = 1").expect("toml default");
        assert_eq!(no_ext.get("x"), Some(&toml::Value::Integer(1)));
        let upper = parse_by_extension(Path::new("b.TOML"), "x = 2").expect("case-insensitive");
        assert_eq!(upper.get("x"), Some(&toml::Value::Integer(2)));
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_by_extension_json() {
        let val = parse_by_extension(Path::new("a.json"), "{\"x\": 1}").expect("json");
        assert_eq!(val.get("x"), Some(&toml::Value::Integer(1)));
    }

    #[cfg(not(feature = "json"))]
    #[test]
    fn json_without_feature_is_parse_error() {
        let err = parse_by_extension(Path::new("a.json"), "{}").expect_err("feature gate");
        assert!(err.to_string().contains("`json` feature"));
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn parse_by_extension_yaml() {
        let val = parse_by_extension(Path::new("a.yaml"), "x: 1").expect("yaml");
        assert_eq!(val.get("x"), Some(&toml::Value::Integer(1)));
        let val = parse_by_extension(Path::new("a.yml"), "x: 2").expect("yml");
        assert_eq!(val.get("x"), Some(&toml::Value::Integer(2)));
    }

    #[cfg(not(feature = "yaml"))]
    #[test]
    fn yaml_without_feature_is_parse_error() {
        let err = parse_by_extension(Path::new("a.yaml"), "x: 1").expect_err("feature gate");
        assert!(err.to_string().contains("`yaml` feature"));
    }
}
