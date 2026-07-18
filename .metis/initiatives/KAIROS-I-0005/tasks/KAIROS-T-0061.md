---
id: service-account-api-keys-docs
level: task
title: "Service-account API keys: docs"
short_code: "KAIROS-T-0061"
created_at: 2026-07-17T22:31:40.469077+00:00
updated_at: 2026-07-18T09:21:07.618424+00:00
parent: KAIROS-I-0005
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0005
---

# Service-account API keys: docs

## Parent Initiative

[[KAIROS-I-0005]] — implements [[KAIROS-A-0017]].

## Objective **[REQUIRED]**

Operator + developer docs for service accounts and API keys: the create → grant →
mint → use flow, security guidance, and a note that skills-plugin agents can use a
key instead of interactive OAuth. Depends on [[KAIROS-T-0059]]/[[KAIROS-T-0060]].

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] A "Service accounts & API keys" section (README or a `docs/` page) covering:
      what a service account is (A-0017), how to create one (API + CLI), grant it
      board capabilities (least privilege), mint a key (raw shown once), and use
      the key with `curl` (`Authorization: Bearer kairos_sk_…`), the CLI, and
      MCP.
- [ ] Security guidance: keys are org-scoped and hashed at rest; rotate via
      mint-new + revoke-old; set `expires_at`; never commit keys; revocation is
      immediate; service accounts can't be org/deployment admins.
- [ ] A pointer in the skills-plugin bootstrap docs: an agent/CI can authenticate
      MCP with a service-account key (non-interactive) instead of the OAuth flow.
- [ ] Cross-links: A-0017, the SCIM-token docs (sibling mechanism), and the
      Google Workspace SSO section (humans vs machines).
- [ ] Prose only — no gate beyond doc build/link sanity; but land after the
      feature so commands/paths documented are real.

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Keep it example-driven (copy-pasteable curl + CLI). Mirror the tone of the
  existing Google Workspace runbook in README.

### Dependencies
- [[KAIROS-T-0059]] (API), [[KAIROS-T-0060]] (CLI) — document the real surface.

### Risk Considerations
- Ensure examples never show a real-looking key that could be mistaken for a
  live secret; use obvious placeholders.

## Status Updates **[REQUIRED]**

### 2026-07-18 — Complete

- README: new "Service accounts & API keys" section — what a service account is
  (A-0017), create (CLI + curl), grant least-privilege capabilities, mint a key
  (shown once), use it as a Bearer on `/api`/`/mcp` (no tenant header — key
  carries its tenant), rotate/revoke/expire, and security notes.
- Skills-plugin bootstrap SKILL.md: note that CI/headless agents can use a
  service-account key as the MCP bearer instead of the browser OAuth flow.
- Placeholders only (no real-looking secrets). Cross-references the Google
  Workspace section (humans) vs keys (machines).

Prose-only; no gate beyond the existing suites (unaffected). Final task of
[[KAIROS-I-0005]].