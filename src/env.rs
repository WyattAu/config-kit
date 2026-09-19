//! Environment-variable layer: prefix matching, key mapping, type
//! inference.

use std::collections::BTreeMap;
use std::env;

use crate::error::ConfigError;

/// Collects every environment variable under `prefix` into a TOML table.
///
/// The prefix is stripped and the remainder lowercased to form the
/// top-level key: `APP_PORT` with prefix `APP_` becomes `port`. Values are
/// typed by inference so plain structs deserialize without ceremony:
/// `true`/`false` become booleans, integer literals become `i64`, float
/// literals become `f64`, and anything else is taken verbatim as a string.
///
/// Returns the table alongside a provenance map from each (lowercased)
/// merged key back to the full environment variable name that supplied it,
/// so type mismatches can be attributed to the variable — not to whichever
/// file the value happened to override.
///
/// Keys that are not valid Unicode can never match a `str` prefix and are
/// skipped.
///
/// # Errors
///
/// [`ConfigError::EnvVar`] when a variable matching `prefix` carries a
/// value that is not valid Unicode.
pub(crate) fn collect(
    prefix: &str,
) -> Result<(toml::Table, BTreeMap<String, String>), ConfigError> {
    let mut table = toml::Table::new();
    let mut sources = BTreeMap::new();
    for (key_os, value_os) in env::vars_os() {
        let Some(key) = key_os.to_str() else {
            continue;
        };
        let Some(rest) = key.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let value = match value_os.into_string() {
            Ok(text) => infer(&text),
            Err(raw) => {
                return Err(ConfigError::EnvVar {
                    key: key.to_owned(),
                    source: env::VarError::NotUnicode(raw),
                });
            }
        };
        let merged_key = rest.to_lowercase();
        table.insert(merged_key.clone(), value);
        sources.insert(merged_key, key.to_owned());
    }
    Ok((table, sources))
}

/// Infers a typed TOML value from a raw environment string.
fn infer(raw: &str) -> toml::Value {
    if raw == "true" {
        return toml::Value::Boolean(true);
    }
    if raw == "false" {
        return toml::Value::Boolean(false);
    }
    if let Ok(int) = raw.parse::<i64>() {
        return toml::Value::Integer(int);
    }
    if let Ok(float) = raw.parse::<f64>() {
        return toml::Value::Float(float);
    }
    toml::Value::String(raw.to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn infer_typed_literals() {
        assert_eq!(infer("true"), toml::Value::Boolean(true));
        assert_eq!(infer("false"), toml::Value::Boolean(false));
        assert_eq!(infer("8080"), toml::Value::Integer(8080));
        assert_eq!(infer("-3"), toml::Value::Integer(-3));
        assert_eq!(infer("0.5"), toml::Value::Float(0.5));
        assert_eq!(infer("svc"), toml::Value::String("svc".into()));
        assert_eq!(infer(""), toml::Value::String(String::new()));
    }

    #[test]
    fn collect_maps_and_lowercases() {
        let marker = "CKIT_TEST_COLLECT_";
        env::set_var(format!("{marker}PORT"), "9000");
        env::set_var(format!("{marker}name"), "demo");
        let (table, sources) = collect(marker).expect("collect");
        assert_eq!(table.get("port"), Some(&toml::Value::Integer(9000)));
        assert_eq!(table.get("name"), Some(&toml::Value::String("demo".into())));
        assert!(table.get("port").is_some());
        // Provenance maps the merged key back to the full variable name.
        assert_eq!(
            sources.get("port"),
            Some(&format!("{marker}PORT")),
            "merged keys must trace back to their variable"
        );
        env::remove_var(format!("{marker}PORT"));
        env::remove_var(format!("{marker}name"));
    }

    #[test]
    fn collect_skips_bare_prefix() {
        let marker = "CKIT_TEST_BARE_";
        env::set_var(marker, "1");
        let (table, sources) = collect(marker).expect("collect");
        assert!(table.is_empty());
        assert!(sources.is_empty());
        env::remove_var(marker);
    }
}
