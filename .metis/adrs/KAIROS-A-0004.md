---
id: 001-content-versioning-with-optimistic
level: adr
title: "Content Versioning with Optimistic Concurrency"
number: 1
short_code: "KAIROS-A-0004"
created_at: 2026-03-04T02:06:54.308467+00:00
updated_at: 2026-07-08T15:00:15.682937+00:00
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

# ADR-4: Content Versioning with Optimistic Concurrency

## Context

Kairos is a multi-user, multi-client system (CLI, GUI, MCP agents). Multiple people or agents may view and edit the same document concurrently. Two problems need solving:

1. **Versioning**: When content changes, what was the previous state? Who changed it? When? This matters for audit, rollback, and understanding how a document evolved.

2. **Concurrent editing**: When two users edit the same document simultaneously, how do we prevent silent overwrites?

These concerns apply specifically to **markdown content** (the `title` and `content` fields on entity tables). Structured metadata (priority, status, etc. in `item_metadata` per KAIROS-A-0003) is not contentious - it's discrete field updates, not free-form text that can be partially overlapping.

## Decision

**Append-only content history with optimistic concurrency control. Metadata changes are not versioned.**

### Schema

Add to each entity table (strategies, initiatives, tasks, documents, adrs):

```
  - version     (integer, starts at 1, incremented on content/title change)
  - updated_by  (uuid, FK -> public.users)
```

One shared history table across all entity types:

```
item_history
  - id          (uuid)
  - item_id     (uuid)
  - version     (integer)
  - title       (text, snapshot at this version)
  - content     (text, snapshot at this version)
  - edited_by   (uuid, FK -> public.users)
  - edited_at   (timestamp)
```

### Optimistic Concurrency Flow

1. Client loads an item. Response includes current `version` number.
2. Client submits a content edit, including the `version` it was based on: "I'm editing version 3."
3. Server checks: does the item's current `version` match the submitted version?
   - **Yes**: Increment version, update title/content, write snapshot to `item_history`. Return new version number.
   - **No**: Reject with HTTP 409 Conflict. Client must reload, reconcile, and retry.
4. Conflict resolution is client-side. The API provides the current version's content; the client decides how to merge or overwrite.

### What Is Versioned

- **Title and content changes**: Every edit increments `version` and creates an `item_history` row with a full snapshot.
- **Metadata changes** (`item_metadata`): Not versioned. Writes go directly. No conflict check. Metadata fields are discrete values (enums, dates, strings) where last-write-wins is acceptable.
- **Column/board transitions**: Not versioned in `item_history`. These are state changes, not content changes. Tracked in `activity_log` instead.

### Activity Log (Non-Content Audit Trail)

Content changes are captured by `item_history`. All other significant actions are captured in `activity_log`:

```
activity_log
  - id          (uuid)
  - actor_id    (uuid - who did it)
  - action      (text - 'transition', 'create', 'delete', 'relationship_add',
                        'relationship_remove', 'capability_grant', 'capability_revoke')
  - entity_id   (uuid, nullable - the item acted on)
  - entity_type (text - 'strategy', 'initiative', 'task', 'document', 'adr')
  - details     (text - structured context, e.g., "column:Draft->Active")
  - occurred_at (timestamp)
```

This provides a complete audit trail: `item_history` tells you what the content was at any point, `activity_log` tells you when items moved between columns, who created/deleted relationships, and who granted/revoked board capabilities.

### History Access

- View full history of any item by querying `item_history WHERE item_id = X ORDER BY version`
- Diff any two versions by comparing snapshots
- Rollback by copying a historical snapshot back to the entity table (creates a new version)

### History Retention: Sweeper, Compaction, and Offload

*(Added 2026-07-08 at ratification, per Dylan: history and log growth must be bounded.)*

`item_history` and `activity_log` are append-only and must stay bounded. A background **sweeper** (in-process scheduled task in the server binary, iterating tenant schemas; same mechanism as the soft-delete purge in KAIROS-A-0001) enforces a tiered retention policy per tenant:

1. **Hot window** (default 90 days, configurable): every version snapshot and every activity row is retained untouched.
2. **Compaction** (past the hot window): `item_history` is thinned to boundary snapshots — the first and last version per item per calendar month are kept; intermediate versions become offload candidates. The latest N versions of any item (default 5) are never compacted regardless of age, so rollback always has recent material.
3. **Offload before prune**: rows leaving the database are first exported as NDJSON to a configured archive target (filesystem path or S3-compatible URL). Only after a successful archive write are the rows deleted. If no archive target is configured, compaction pauses and a warning is logged/metered — data is never destroyed without an offload copy unless the operator explicitly sets the retention mode to `discard`.
4. **`activity_log`**: same pattern without compaction tiers — archive-then-delete rows older than the retention window (default 365 days).

Configuration (env, per KAIROS-A-0013): `KAIROS_HISTORY_HOT_DAYS`, `KAIROS_HISTORY_KEEP_LATEST`, `KAIROS_ACTIVITY_RETENTION_DAYS`, `KAIROS_ARCHIVE_TARGET`, `KAIROS_RETENTION_MODE` (archive|discard|off). Sweeper runs are themselves recorded in `activity_log` (action: `retention_sweep`, with row counts) and surfaced as Prometheus metrics.

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **A: Last-write-wins, no versioning** | Simplest, no extra tables | Silent overwrites, no audit trail, no rollback | High | Low |
| **B: Last-write-wins with history** | Audit trail, rollback possible | Silent overwrites still happen, history captures the damage but doesn't prevent it | Medium | Low |
| **C: Optimistic concurrency with history** (chosen) | Prevents silent overwrites, full audit trail, rollback, simple implementation | Clients must handle 409 conflicts, slight overhead of version check per write | Low | Medium |
| **D: Pessimistic locking** | Guarantees exclusive editing | Lock management complexity, abandoned locks, blocks legitimate concurrent work, poor UX | Medium | High |
| **E: OT/CRDTs** | Real-time collaborative editing | Massive complexity, requires WebSocket infrastructure, overkill for this system | Low | Very High |

## Rationale

1. **Optimistic concurrency is the right trade-off.** Most edits don't conflict. When they do, the user is informed immediately rather than silently losing work. The overhead (one integer comparison per write) is negligible.

2. **Full snapshots over diffs.** Snapshots are larger but trivial to query, compare, and restore. Diff-based history requires replaying a chain to reconstruct any version - fragile and slow. Storage cost for text content in a planning tool is minimal.

3. **Metadata doesn't need versioning.** Changing priority from "medium" to "high" is not contentious. It's a discrete field update. Versioning it would add noise to the history without meaningful benefit.

4. **One history table for all types.** The shared UUID space means `item_history` works across strategies, initiatives, tasks, documents, and ADRs without type discrimination. Simple.

5. **Client-side conflict resolution.** The server's job is to detect conflicts and reject. The client decides how to resolve - show both versions, auto-merge, or let the user pick. This keeps the server stateless and simple.

## Consequences

### Positive
- No silent overwrites - concurrent edits are detected and surfaced
- Full audit trail of every content change with who and when
- Rollback to any previous version is possible
- Simple implementation - one integer comparison on write, one INSERT to history
- Metadata operations remain fast with no versioning overhead

### Negative
- Clients must handle 409 Conflict responses (reload, show diff, retry)
- History table grows with every content edit (bounded by the retention sweeper: hot window → monthly boundary compaction → archive-then-prune)
- No real-time collaborative editing - if two users are editing simultaneously, one will get a conflict on save

### Neutral
- MCP agents (AI) will need to handle conflicts the same way human clients do - load version, edit, submit with version, handle rejection