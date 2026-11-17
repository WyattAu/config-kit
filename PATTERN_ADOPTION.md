# PATTERN_ADOPTION

Relationship of config-kit to the WyattAu pattern canon
(WyattAu/pattern-standards). Statuses and evidence format:
`references/adoption-format.md`. Validated by
`scripts/adoption-check.py --file PATTERN_ADOPTION.md`.

| Pattern | Status | Evidence |
|---|---|---|
| T1 | adopted | src/builder.rs:171 (load deserializes the merged value into T exactly once, parse errors attribute to the supplying layer via provenance), src/env.rs:13 (env values typed by inference, no stringly-typed re-validation) |
| T2 | adopted | src/builder.rs:19 (Empty/Ready typestate: zero-layer load is a compile error, not a runtime failure), src/builder.rs:105 (layer() is the only state transition) |
| T13 | adopted | src/builder.rs:263 (the merge is a pure fold over values with provenance tracked in-band); IO is confined to src/builder.rs:352 (read_layer_value) and the watcher worker src/watcher.rs:174 |
| P4 | adopted | src/sensitive.rs:32 (Sensitive renders redacted output for Debug and Display), src/sensitive.rs:45 (expose() is the only read path) |
| P47 | partial | src/watcher.rs:202 (a reload hands the handler an owned parsed snapshot; a failed reload keeps the prior config in effect, src/watcher.rs:34). No epoch-pinned request-bound reader API: consumers pin an epoch by holding the handed-off value |
| P70 | adopted | src/builder.rs:154 (load ignores unknown keys by default, so a config that gains fields does not break older readers); denial is opt-in via load_strict at src/builder.rs:222 |
