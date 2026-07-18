---
id: m0-ci-skeleton-running-the-angreal
level: task
title: "M0: CI skeleton running the angreal gates"
short_code: "KAIROS-T-0005"
created_at: 2026-07-08T15:06:00.494353+00:00
updated_at: 2026-07-09T17:18:00.335833+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0004]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M0: CI skeleton running the angreal gates

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

CI skeleton (GitHub Actions) that runs the A-0012 gates on every push/PR via angreal, so the agent gate and CI are the same commands.

## Acceptance Criteria

## Acceptance Criteria

- [x] Workflow: checkout → toolchain → docker available → `cargo fmt --check` → `cargo clippy --workspace -- -D warnings` → `angreal test unit` → `angreal test integration`
- [x] Workflow YAML validated (actionlint or equivalent); job matrix/caching for cargo configured
- [x] README/Status Update documents the activation step: create GitHub remote, push, enable branch protection requiring the workflow

## Implementation Notes

The repo has no remote yet — this task delivers a committed, validated workflow; activation happens when Dylan creates the remote. Do not invent a remote.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-09: Implemented. Deliverables:
  - `.github/workflows/ci.yml` — single sequential job `ci` on push-to-main + pull_request; timeout 30m; per-ref concurrency with cancel-in-progress; `permissions: contents: read`. Steps: actions/checkout@v4 → `rustup toolchain install` (reads `rust-toolchain.toml`: 1.93.0 + rustfmt/clippy) → Swatinem/rust-cache@v2 → docker/compose availability check → libpq-dev install (diesel postgres backend links libpq) → setup-python 3.12 → `pip install "angreal==2.8.8"` (PyPI package name verified against local install, angreal v2.8.8) → gates in order: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `angreal test unit`, `angreal test integration`. No services config in the workflow — the integration task drives compose up --wait / down -v itself per `.angreal/task_test.py`.
  - `README.md` (new — none existed) — project one-liner, angreal-driven dev workflow, "## CI" section listing the four gates and the one-time activation steps: create GitHub remote, `git push -u origin main`, enable branch protection on `main` requiring the `ci` check. No remote invented; no push attempted.
- 2026-07-09: Verification evidence (all at repo root):
  - actionlint containerized: `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color` → no findings, exit 0.
  - YAML parse: `python -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` (via uv+pyyaml) → "YAML OK".
  - `cargo fmt --check` → clean, exit 0.
  - `cargo clippy --workspace --all-targets -- -D warnings` → "Finished dev profile", exit 0.
  - `angreal test unit` → all workspace smoke tests pass ("test result: ok" for each crate), exit 0.
  - `angreal test integration` → not run in this session per orchestrator instruction (orchestrator runs the full tier); command existence confirmed via `angreal test --help`, compose-self-managing lifecycle confirmed in `.angreal/task_test.py`.
  - Criterion 2 note: single job, no matrix — the A-0012 gates are sequential by design; cargo caching provided by Swatinem/rust-cache@v2. Clippy uses `--all-targets`, a superset of the criterion's invocation.