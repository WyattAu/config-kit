#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration tests: the full layer → merge → load surface, strict
//! unknown-key denial, secret redaction, typed env parsing, and (behind
//! the feature) hot reload.

use config_kit::{ConfigBuilder, ConfigError, ConfigLayer, Sensitive};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize, Debug, PartialEq)]
struct Service {
    region: String,
    port: u16,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)] // fields are consumed by serde, not read directly
struct OnlyPort {
    port: u16,
}

fn overrides(pairs: &[(&str, toml::Value)]) -> BTreeMap<String, toml::Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

#[test]
fn env_type_mismatch_is_attributed_to_the_env_var() {
    // Regression (estate-integration round 3): a value supplied by an
    // env layer that fails to type-check used to be reported against the
    // last FILE layer read. The error must name the variable itself.
    let prefix = "CKIT_ATTR_ENV_";
    std::env::set_var(format!("{prefix}PORT"), "not-a-number");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("base.toml");
    std::fs::write(&path, "port = 1\n").expect("write");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path)) // last file layer read...
        .layer(ConfigLayer::env_prefix(prefix)) // ...but the bad value is ours
        .load::<OnlyPort>()
        .expect_err("string port cannot deserialize into u16");

    let rendered = error.to_string();
    assert!(
        rendered.contains(&format!("<env:{prefix}PORT>")),
        "error must name the environment variable, got: {rendered}"
    );
    assert!(
        !rendered.contains("base.toml"),
        "the file that was overridden must not be blamed: {rendered}"
    );
    std::env::remove_var(format!("{prefix}PORT"));
}

#[test]
fn file_type_mismatch_is_attributed_to_its_file() {
    // Symmetry: with no overriding layer, a bad file value names the file
    // — and not a later, unrelated file layer.
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path().join("base.toml");
    std::fs::write(&base, "port = \"not-a-number\"\n").expect("write");
    let other = dir.path().join("other.toml");
    std::fs::write(&other, "region = \"eu\"\n").expect("write");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(&base)) // carries the bad value
        .layer(ConfigLayer::file(&other)) // read last — must NOT be blamed
        .load::<Service>()
        .expect_err("string port cannot deserialize into u16");

    let rendered = error.to_string();
    assert!(rendered.contains("base.toml"), "got: {rendered}");
    assert!(!rendered.contains("other.toml"), "got: {rendered}");
}

#[test]
fn override_type_mismatch_is_attributed_to_overrides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("base.toml");
    std::fs::write(&path, "port = 1\n").expect("write");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .layer(ConfigLayer::overrides(overrides(&[(
            "port",
            toml::Value::String("not-a-number".into()),
        )])))
        .load::<OnlyPort>()
        .expect_err("string port cannot deserialize into u16");

    assert!(
        error.to_string().contains("<overrides>"),
        "error must name the overrides layer, got: {error}"
    );
}

#[test]
fn precedence_overrides_beat_env_beat_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("service.toml");
    std::fs::write(&path, "region = \"file\"\nport = 1000\n").expect("write");

    let prefix = "CKIT_PRECEDENCE_";
    std::env::set_var(format!("{prefix}REGION"), "env");
    std::env::set_var(format!("{prefix}PORT"), "2000");

    let service: Service = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .layer(ConfigLayer::env_prefix(prefix))
        .layer(ConfigLayer::overrides(overrides(&[
            ("region", toml::Value::String("override".into())),
            ("port", toml::Value::Integer(3000)),
        ])))
        .load()
        .expect("layered load");

    assert_eq!(
        service,
        Service {
            region: "override".into(),
            port: 3000,
        }
    );
    std::env::remove_var(format!("{prefix}REGION"));
    std::env::remove_var(format!("{prefix}PORT"));
}

#[test]
fn env_beats_file_without_overrides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("service.toml");
    std::fs::write(&path, "region = \"file\"\nport = 1000\n").expect("write");

    let prefix = "CKIT_ENV_OVER_FILE_";
    std::env::set_var(format!("{prefix}PORT"), "2000");

    let service: Service = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .layer(ConfigLayer::env_prefix(prefix))
        .load()
        .expect("layered load");

    assert_eq!(
        service,
        Service {
            region: "file".into(),
            port: 2000,
        }
    );
    std::env::remove_var(format!("{prefix}PORT"));
}

#[derive(Deserialize, Debug, PartialEq)]
struct DbConfig {
    url: String,
    port: u16,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Root {
    db: DbConfig,
    flag: i64,
}

#[test]
fn deep_merge_preserves_sibling_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("db.toml");
    std::fs::write(
        &path,
        "[db]\nurl = \"postgres://base\"\nport = 5432\n\nflag = [1, 2]\n",
    )
    .expect("write");

    let mut db_over = toml::Table::new();
    db_over.insert("port".into(), toml::Value::Integer(6543));
    let mut over = BTreeMap::new();
    over.insert("db".into(), toml::Value::Table(db_over));
    over.insert("flag".into(), toml::Value::Integer(9));

    let root: Root = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .layer(ConfigLayer::overrides(over))
        .load()
        .expect("merged load");

    assert_eq!(
        root,
        Root {
            db: DbConfig {
                url: "postgres://base".into(), // sibling survived the merge
                port: 6543,                    // nested override applied
            },
            flag: 9, // non-table value replaced wholesale
        }
    );
}

#[test]
fn missing_file_is_io_error() {
    let missing = std::path::Path::new("/nonexistent/ckit/service.toml");
    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(missing))
        .load::<toml::Value>()
        .expect_err("missing file");
    assert!(
        matches!(error, ConfigError::Io { .. } if error.to_string().contains("service.toml")),
        "{error}"
    );
}

#[test]
fn malformed_file_is_parse_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("broken.toml");
    std::fs::write(&path, "port = 1000\nregion =\nbroken").expect("write");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load::<toml::Value>()
        .expect_err("malformed file");
    assert!(matches!(error, ConfigError::Parse { .. }), "{error}");
    assert!(error.to_string().contains("broken.toml"));
}

#[derive(Deserialize, Debug)]
struct StrictSettings {
    db: DbConfig,
}

#[test]
fn strict_load_reports_unknown_top_level_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("strict.toml");
    std::fs::write(
        &path,
        "typo_b = 1\ntypo_a = 2\n\n[db]\nurl = \"postgres://x\"\nport = 1\n",
    )
    .expect("write");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load_strict::<StrictSettings>()
        .expect_err("unknown keys");
    assert!(
        matches!(&error, ConfigError::UnknownFields { keys }
            if keys == &["typo_a".to_owned(), "typo_b".to_owned()]),
        "{error}"
    );
}

#[test]
fn strict_load_passes_clean_configuration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("clean.toml");
    std::fs::write(&path, "[db]\nurl = \"postgres://x\"\nport = 1\n").expect("write");

    let settings: StrictSettings = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load_strict()
        .expect("strict load");
    assert_eq!(settings.db.url, "postgres://x");
}

#[test]
fn strict_denial_is_top_level_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested.toml");
    // `rogue_nested` is unknown but nested — deliberately out of scope for
    // the container-agnostic top-level check.
    std::fs::write(
        &path,
        "[db]\nurl = \"postgres://x\"\nport = 1\nrogue_nested = true\n",
    )
    .expect("write");

    let settings: StrictSettings = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load_strict()
        .expect("nested unknowns are not top-level");
    assert_eq!(settings.db.port, 1);
}

#[test]
fn strict_denial_covers_env_and_override_keys() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)] // fields are consumed by serde, not read directly
    struct Flat {
        db_url: String,
        db_port: u16,
    }

    let prefix = "CKIT_STRICT_ENV_";
    std::env::set_var(format!("{prefix}DB_URL"), "postgres://x");
    std::env::set_var(format!("{prefix}DB_PORT"), "1");
    std::env::set_var(format!("{prefix}SURPLUS"), "1");

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::env_prefix(prefix))
        .load_strict::<Flat>()
        .expect_err("surplus env key");
    assert!(
        matches!(&error, ConfigError::UnknownFields { keys } if keys == &["surplus".to_owned()]),
        "{error}"
    );

    let mut over = BTreeMap::new();
    over.insert("db".into(), toml::Value::Boolean(true)); // wrong shape: db is a table
    let error = ConfigBuilder::new()
        .layer(ConfigLayer::overrides(over))
        .load_strict::<StrictSettings>()
        .expect_err("override replaces the db table");
    assert!(matches!(error, ConfigError::Parse { .. }), "{error}");
    // 0.1.1: the offending value's layer is attributed — `<overrides>`
    // here, where 0.1.0 fell back to `(merged)`.
    assert!(error.to_string().contains("<overrides>"));
    std::env::remove_var(format!("{prefix}DB_URL"));
    std::env::remove_var(format!("{prefix}DB_PORT"));
    std::env::remove_var(format!("{prefix}SURPLUS"));
}

#[derive(Deserialize, Debug, PartialEq)]
struct Typed {
    port: u16,
    debug: bool,
    ratio: f64,
    name: String,
}

#[test]
fn env_values_are_typed_via_serde() {
    let prefix = "CKIT_TYPED_";
    std::env::set_var(format!("{prefix}PORT"), "8080");
    std::env::set_var(format!("{prefix}DEBUG"), "true");
    std::env::set_var(format!("{prefix}RATIO"), "0.5");
    std::env::set_var(format!("{prefix}NAME"), "svc");

    let typed: Typed = ConfigBuilder::new()
        .layer(ConfigLayer::env_prefix(prefix))
        .load()
        .expect("typed env load");

    assert_eq!(
        typed,
        Typed {
            port: 8080,
            debug: true,
            ratio: 0.5,
            name: "svc".into(),
        }
    );
    for key in ["PORT", "DEBUG", "RATIO", "NAME"] {
        std::env::remove_var(format!("{prefix}{key}"));
    }
}

#[test]
#[cfg(unix)]
fn non_unicode_env_value_is_envvar_error() {
    use std::os::unix::ffi::OsStringExt;

    let prefix = "CKIT_NONUNICODE_";
    std::env::set_var(
        std::ffi::OsString::from_vec(format!("{prefix}TOKEN").into_bytes()),
        std::ffi::OsString::from_vec(vec![0xff, 0xfe]),
    );

    let error = ConfigBuilder::new()
        .layer(ConfigLayer::env_prefix(prefix))
        .load::<toml::Value>()
        .expect_err("non-unicode env value");
    assert!(
        matches!(&error, ConfigError::EnvVar { key, .. } if key == &format!("{prefix}TOKEN")),
        "{error}"
    );
    std::env::remove_var(std::ffi::OsString::from_vec(
        format!("{prefix}TOKEN").into_bytes(),
    ));
}

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
struct WithSecret {
    token: Sensitive<String>,
    port: u16,
}

#[test]
fn sensitive_redacts_through_containers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("secret.toml");
    std::fs::write(&path, "token = \"super-secret-value\"\nport = 1\n").expect("write");

    let config: WithSecret = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load()
        .expect("load with secret");

    let rendered = format!("{config:?}");
    assert!(rendered.contains("[REDACTED]"), "{rendered}");
    assert!(!rendered.contains("super-secret-value"), "{rendered}");
    assert_eq!(config.token.expose(), "super-secret-value");
    let rendered = format!("{}", config.token);
    assert_eq!(rendered, "[REDACTED]");
}

#[test]
#[cfg(feature = "serde")]
fn sensitive_serde_roundtrip_preserves_the_secret() {
    let original: WithSecret = toml::from_str("token = \"rotate-me\"\nport = 2").expect("load");
    let encoded = toml::to_string(&original).expect("serialize");
    let decoded: WithSecret = toml::from_str(&encoded).expect("reload");
    assert_eq!(decoded.token.expose(), "rotate-me");
    assert_eq!(format!("{:?}", decoded.token), "[REDACTED]");
}

#[cfg(feature = "json")]
#[test]
fn json_layers_load_by_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("service.json");
    std::fs::write(&path, r#"{"region": "json", "port": 4}"#).expect("write");

    let service: Service = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load()
        .expect("json load");
    assert_eq!(service.region, "json");
    assert_eq!(service.port, 4);
}

#[cfg(feature = "yaml")]
#[test]
fn yaml_layers_load_by_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("service.yaml");
    std::fs::write(&path, "region: yaml\nport: 5\n").expect("write");

    let service: Service = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load()
        .expect("yaml load");
    assert_eq!(service.region, "yaml");
    assert_eq!(service.port, 5);
}

#[test]
fn files_without_extension_default_to_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("defaults");
    std::fs::write(&path, "region = \"toml\"\nport = 6\n").expect("write");

    let service: Service = ConfigBuilder::new()
        .layer(ConfigLayer::file(&path))
        .load()
        .expect("toml default load");
    assert_eq!(service.region, "toml");
}

#[cfg(feature = "hot-reload")]
mod hot_reload {
    use config_kit::{watch, ConfigError};
    use serde::Deserialize;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[derive(Deserialize, Debug, Clone, PartialEq)]
    struct Toggle {
        enabled: bool,
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }

    #[test]
    fn callback_fires_on_write_within_two_seconds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("toggle.toml");
        std::fs::write(&path, "enabled = false\n").expect("write");

        let seen: Arc<Mutex<Option<Toggle>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&seen);
        let watcher = watch(&path, move |toggle: Toggle| {
            *slot.lock().expect("slot lock") = Some(toggle);
        })
        .expect("watch starts");

        std::fs::write(&path, "enabled = true\n").expect("rewrite");

        let limit = deadline();
        loop {
            let observed = seen.lock().expect("slot lock").clone();
            if observed == Some(Toggle { enabled: true }) {
                break;
            }
            assert!(Instant::now() < limit, "callback did not fire within 2s");
            std::thread::sleep(Duration::from_millis(50));
        }
        watcher.stop();
    }

    #[test]
    fn reload_failures_are_recorded_not_fatal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("toggle.toml");
        std::fs::write(&path, "enabled = false\n").expect("write");

        let watcher = watch::<Toggle, _>(&path, |_| {}).expect("watch starts");
        std::fs::write(&path, "{{{ not toml").expect("rewrite");

        let limit = deadline();
        loop {
            if watcher.last_error().is_some() {
                break;
            }
            assert!(Instant::now() < limit, "parse error not recorded within 2s");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(watcher.last_error().is_some());
        watcher.stop();
    }

    #[test]
    fn missing_path_is_watch_failed() {
        let error = watch::<Toggle, _>(Path::new("/nonexistent/ckit/toggle.toml"), |_| {})
            .expect_err("missing path");
        assert!(matches!(error, ConfigError::WatchFailed { .. }), "{error}");
    }
}
