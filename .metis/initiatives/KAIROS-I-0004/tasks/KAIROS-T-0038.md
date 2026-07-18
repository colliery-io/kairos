---
id: m4-cli-packaging-and-release
level: task
title: "M4: CLI - packaging and release binaries"
short_code: "KAIROS-T-0038"
created_at: 2026-07-10T22:02:38.462257+00:00
updated_at: 2026-07-14T23:29:22.029551+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0037]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: CLI - packaging and release binaries

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

CLI distribution per A-0015: release binaries and install documentation.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] CI release workflow (tag-triggered) builds `kairos` for macOS arm64/x86_64 and Linux x86_64/arm64, attaches binaries to the GitHub release
- [x] `cargo install --path crates/kairos-cli` documented; README section for CLI install + quickstart (login → whoami → boards)
- [x] Version output (`kairos --version`) matches the workspace version; release workflow validated with actionlint (containerized)

## Implementation Notes

References A-0015, A-0013 (release conventions). Keep the workflow separate from ci.yml (release.yml).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-14: Implemented `.github/workflows/release.yml` (tag-triggered `v*`). Native matrix builds — no cross toolchain: macos-14 → aarch64-apple-darwin, macos-15-intel → x86_64-apple-darwin (macos-13 is retired per actionlint's 2026 runner-label set), ubuntu-24.04 → x86_64-unknown-linux-gnu, ubuntu-24.04-arm → aarch64-unknown-linux-gnu (GitHub-hosted arm64 runner chosen over cross-compilation). Artifacts `kairos-<version>-<target>.tar.gz` + `.sha256`, attached via softprops/action-gh-release@v2 (`generate_release_notes`, `fail_on_unmatched_files`). Guard step fails the build if the tag version ≠ `[workspace.package]` version or if `kairos --version` ≠ `kairos <version>`. Toolchain from rust-toolchain.toml + Swatinem/rust-cache, mirroring ci.yml. Commented anchor left for the T-0047 image-publish job. LICENSE: none exists at repo root today; packaging step conditionally copies `LICENSE*` when present.
- 2026-07-14: libpq verification — `cargo build -p kairos-cli --release` finished clean with NO diesel/pq-sys compile (`cargo tree -p kairos-cli -e normal` has no diesel/postgres/pq entries; `otool -L target/release/kairos` shows only libSystem + libiconv). kairos-server/kairos-db are dev-dependencies only and are not compiled for the release bin, so release runners install no native Postgres packages.
- 2026-07-14: Evidence — `./target/release/kairos --version` → `kairos 0.1.0` (matches workspace version 0.1.0); binary 6.1 MB (aarch64-apple-darwin host); local dry-run of the packaging step produced `kairos-0.1.0-aarch64-apple-darwin.tar.gz` (2.6 MB, contains `./kairos`) + sha256. Containerized actionlint (`docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color`) exit 0 (first run flagged retired `macos-13`; fixed to `macos-15-intel`). `cargo fmt --check` clean. README gained a bounded `## CLI` section (release-binary curl installs per target, `cargo install --path crates/kairos-cli`, login → whoami → boards quickstart verified against the built binary's `--help` output, exit-code table 0/1/2 per A-0015). No crate sources or ci.yml touched; workflow-only change so clippy/unit/integration gates are unaffected (shared-services mode: integration not re-run here).