//! Criterion benches: layered load throughput (3 layers × 20 keys) and
//! environment-variable parse throughput.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, dead_code)]

use config_kit::{ConfigBuilder, ConfigLayer};
use criterion::{criterion_group, criterion_main, Criterion};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::hint::black_box;
use tempfile::TempDir;

const KEYS: [&str; 20] = [
    "k00", "k01", "k02", "k03", "k04", "k05", "k06", "k07", "k08", "k09", "k10", "k11", "k12",
    "k13", "k14", "k15", "k16", "k17", "k18", "k19",
];

#[derive(Deserialize)]
struct Twenty {
    k00: u16,
    k01: u16,
    k02: u16,
    k03: u16,
    k04: u16,
    k05: u16,
    k06: u16,
    k07: u16,
    k08: u16,
    k09: u16,
    k10: u16,
    k11: u16,
    k12: u16,
    k13: u16,
    k14: u16,
    k15: u16,
    k16: u16,
    k17: u16,
    k18: u16,
    k19: u16,
}

/// Writes a 20-key TOML file and seeds 20 `CKITBENCH_*` env vars.
fn setup_layers() -> (TempDir, std::path::PathBuf, BTreeMap<String, toml::Value>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bench.toml");
    let mut body = String::new();
    for (key, idx) in KEYS.iter().zip(100_i64..) {
        writeln!(body, "{key} = {idx}").expect("write to string");
    }
    std::fs::write(&path, body).expect("write bench file");

    // Justified at the lint site: `i64` does not implement `AsRef<OsStr>`;
    // clippy's `unnecessary_to_owned` suggestion does not compile here.
    #[allow(clippy::unnecessary_to_owned)]
    for (key, idx) in KEYS.iter().zip(200_i64..) {
        std::env::set_var(format!("CKITBENCH_{key}"), idx.to_string());
    }

    let overrides: BTreeMap<String, toml::Value> = KEYS
        .iter()
        .zip(300_i64..)
        .map(|(key, value)| ((*key).to_owned(), toml::Value::Integer(value)))
        .collect();
    (dir, path, overrides)
}

/// Full 3-layer pipeline: file parse + env collect + override merge +
/// serde deserialize.
fn load_three_layers(path: &std::path::Path, overrides: BTreeMap<String, toml::Value>) -> Twenty {
    ConfigBuilder::new()
        .layer(ConfigLayer::file(path))
        .layer(ConfigLayer::env_prefix("CKITBENCH_"))
        .layer(ConfigLayer::overrides(overrides))
        .load()
        .expect("bench load")
}

/// Env-only pipeline: variable collection, inference, serde deserialize.
fn load_env_only() -> Twenty {
    ConfigBuilder::new()
        .layer(ConfigLayer::env_prefix("CKITBENCH_"))
        .load()
        .expect("bench load")
}

fn bench_layer_merge_3x20(c: &mut Criterion) {
    let (_dir, path, overrides) = setup_layers();
    c.bench_function("layer_merge_3x20", |b| {
        b.iter(|| black_box(load_three_layers(black_box(&path), overrides.clone())));
    });
}

fn bench_env_parse_20(c: &mut Criterion) {
    let _guard = setup_layers();
    c.bench_function("env_parse_20", |b| b.iter(|| black_box(load_env_only())));
}

criterion_group!(benches, bench_layer_merge_3x20, bench_env_parse_20);
criterion_main!(benches);
