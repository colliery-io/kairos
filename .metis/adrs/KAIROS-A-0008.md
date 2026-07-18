---
id: 001-kairos-monorepo-service-and-skills
level: adr
title: "Kairos Monorepo - Service and Skills Plugin Distributed Together"
number: 1
short_code: "KAIROS-A-0008"
created_at: 2026-07-08T11:11:53.240920+00:00
updated_at: 2026-07-08T15:00:27.211050+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Kairos Monorepo - Service and Skills Plugin Distributed Together

## Context

Kairos gains a second deliverable alongside the multi-tenant service: an agentic development skills plugin ("beast mode", KAIROS-I-0002) — a Kairos-bound port of mattpocock/skills plus new review/bootstrap skills and distributable specs, shipping with the product from day 1. It needs a home and a distribution channel, and its relationship to the service needed deciding: separate repo, extension of the existing metis plugin, or co-located with the service.

## Decision

The Kairos repo is a monorepo. It contains both the Kairos service (Rust/Axum + Postgres) and the Claude Code skills plugin (`.claude-plugin/` manifest + `skills/` + `references/`). The two legs are one product: the service brings Flight Levels to companies, and the plugin is the agentic workflow layer that ships with it from day 1 — skills bind to Kairos through its MCP server and operate on Kairos boards. The plugin is for Kairos and Kairos alone; Metis remains a separate, untouched personal tool that this repo merely uses internally for planning. Both legs are planned in this repo's `.metis/`.

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Monorepo (chosen) | One program, one repo; plugin dogfoods on this repo's own Metis; single planning instance for both legs | Repo mixes Rust service and markdown plugin concerns; plugin installers pull service code too | Low | Low |
| Standalone plugin repo | Clean distribution surface; small clone for plugin users | Two repos to coordinate; program planning splits across two Metis instances | Medium | Low |
| Extend existing metis plugin | Users get one plugin; no new distribution channel | Couples experimental beast-mode work to a stable plugin others use; Kairos is the forward brand/backend, not Metis | Medium | Medium |

## Rationale

The plugin is product surface, not tooling — Kairos without its skills is an incomplete offering, so the skills belong in the product's repo and release train. Building both legs in one repo means one planning instance, shared vocabulary, and a tight loop: the skills are designed against the same API and board model the service implements, and version together with it. Metis itself is deliberately untouched — it works, and this repo only consumes it internally for planning.

## Consequences

### Positive
- One `.metis/` instance plans both legs of the program; cross-leg dependencies are ordinary `blocked_by` edges
- Plugin development dogfoods on the Kairos service work itself

### Negative
- Plugin users installing from the marketplace clone the whole repo, service code included
- CI and repo conventions must serve two very different artifact types (Rust crates, markdown skills)

### Neutral
- If distribution weight ever becomes a real problem, the plugin subtree can be split or mirrored to a release repo later

## Review Schedule

### Review Triggers
- Plugin adoption by users who have no interest in the service (distribution weight complaint)
- The service moving to its own deployment/release cadence that monorepo CI can't serve