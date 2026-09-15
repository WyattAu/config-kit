# Security Policy — config-kit

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a vulnerability

Report privately via [GitHub security advisories] for this repository, or
email **wyatt_au@protonmail.com**. Do **not** open a public issue for
security reports.

You will receive an acknowledgement within **72 hours**. Coordinated
disclosure: we ask for up to 90 days before public disclosure while a
patch ships.

## Scope notes

`config-kit` parses configuration from files and the process environment.
Security considerations for integrators:

- **Secrets live in your config layer.** `Sensitive<T>` redacts every
  formatting path (`Debug`/`Display` render `"[REDACTED]"` with impls that
  carry no `T` bounds, so derived container formatting cannot leak), but
  it cannot protect the value after `.expose()`/`.into_inner()` — that is
  deliberate, and every read site is greppable (`\.expose\(\)|\.into_inner\(\)`).
  Prefer `Sensitive` for tokens, passwords, and keys; keep them out of
  logs and error types.
- **Serialization of secrets is opt-in** (the `serde` feature enables
  `Serialize` for `Sensitive<T>`) — enable it only for pipelines that must
  write secrets back out.
- **Environment variables are process-global** and often world-readable
  via `/proc/<pid>/environ`; treat env-delivered secrets as lower
  confidentiality than file-delivered ones. Non-Unicode values under your
  prefix surface as a typed error rather than being silently dropped.
- **Hot reload parses attacker-renameable paths** if you point `watch` at
  a directory others can write; watch files your service owns. Reload
  failures never crash the process — the last valid configuration stays in
  effect — so a failed open (denial) is the worst case, not code
  execution.
- Configuration files drive typed deserialization only; there is no
  template evaluation, command execution, or path traversal in any layer.
- `#![forbid(unsafe_code)]` — no unsafe blocks exist in this crate.

[GitHub security advisories]:
    https://github.com/WyattAu/config-kit/security/advisories/new
