---
id: m0-cargo-workspace-scaffold-six
level: task
title: "M0: Cargo workspace scaffold - six crates"
short_code: "KAIROS-T-0002"
created_at: 2026-07-08T15:05:50.848457+00:00
updated_at: 2026-07-09T12:38:05.589661+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0001]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M0: Cargo workspace scaffold - six crates

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Stand up the cargo workspace per KAIROS-A-0009 (decided): six crates compiling, linting, and testing green — the skeleton every subsequent task builds inside.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Workspace `Cargo.toml` with members `crates/kairos-core`, `crates/kairos-db`, `crates/kairos-server`, `crates/kairos-client`, `crates/kairos-cli`, `crates/kairos-web`; shared `[workspace.dependencies]` with pinned versions: axum, tokio, tower, tower-http, diesel, diesel-async, bb8, thiserror, serde, tracing, tracing-subscriber, clap, leptos, aurora-dark (kairos-web only), utoipa, rmcp
- [x] `rust-toolchain.toml` committed; `rustfmt.toml`/clippy config as needed
- [x] `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` all pass (each crate has a placeholder lib/bin + one smoke test)

## Implementation Notes

References KAIROS-A-0009 for crate responsibilities. kairos-web only needs to compile as a lib placeholder at this stage (cargo-leptos wiring lands with KAIROS-I-0004). Pin diesel-async deliberately (A-0009 consequence).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Scaffolded workspace per KAIROS-A-0009: root `Cargo.toml` (resolver "2", six members under `crates/`), `rust-toolchain.toml` pinning `channel = "1.93.0"` (current stable, rustc 1.93.0) with rustfmt+clippy components. Default rustfmt/clippy behavior is sufficient at this stage — no `rustfmt.toml`/`clippy.toml` needed.
- 2026-07-09: Versions pinned in `[workspace.dependencies]` (looked up via `cargo search`/crates.io index on 2026-07-08): axum 0.8.9, tokio 1.52.3 (features full), tower 0.5.3, tower-http 0.7.0, diesel 2.3.10 (features postgres), diesel-async **=0.9.2** exact-pinned per A-0009 consequence (features postgres, bb8), bb8 0.9.1, thiserror 2.0.18, serde 1.0.228 (derive), serde_json 1.0.150, tracing 0.1.44, tracing-subscriber 0.3.23, clap 4.6.1 (derive), leptos 0.8.20 (latest stable; 0.9.0-alpha exists but is a pre-release), utoipa 5.5.0, rmcp 2.2.0.
- 2026-07-09: **aurora-dark note**: no crate named `aurora-dark` exists on crates.io. It is published as `colliery-io-aurora` 0.1.0 (repo github.com/colliery-io/aurora-dark, "Aurora Dark — Colliery's general dark design system for Leptos", requires leptos ^0.8 — compatible with our 0.8.20 pin). Wired as a rename: `aurora-dark = { package = "colliery-io-aurora", version = "0.1.0" }`, used by kairos-web only. Compiles clean.
- 2026-07-09: Per-crate deps kept minimal: core→thiserror+serde; db→diesel+diesel-async+bb8+thiserror; server(bin)→axum+tokio+tower+tracing+core+db; client→serde+thiserror; cli(bin `kairos`)→clap+client; web→leptos+aurora-dark. Each crate has a doc comment naming its A-0009 responsibility and one `smoke` test; binaries print name+version and exit 0.
- 2026-07-09: Evidence — `cargo build --workspace` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 3m 05s` (exit 0, no libpq issue with diesel `postgres` feature; nothing installed — no Homebrew used, per constraint).
- 2026-07-09: Evidence — `cargo test --workspace` → 6 test binaries, each `test tests::smoke ... ok`, all `test result: ok. 1 passed; 0 failed` (+4 empty doc-test suites), exit 0.
- 2026-07-09: Evidence — `cargo fmt --check` → clean (exit 0). `cargo clippy --workspace -- -D warnings` → `Finished \`dev\` profile ... in 4m 10s`, zero warnings (exit 0).
- 2026-07-09: Evidence — `cargo run -q -p kairos-server` → `kairos-server 0.1.0`, exit 0; `cargo run -q -p kairos-cli` → `kairos-cli 0.1.0`, exit 0.
- 2026-07-09: Verification-gate note: `angreal test unit`/`angreal test integration` tasks are not yet wired to the cargo workspace (angreal predates the Rust scaffold); cargo equivalents run directly above. All acceptance criteria met → completing.