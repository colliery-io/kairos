---
id: 001-template-system-with-reusable
level: adr
title: "Template System with Reusable Typed Metadata"
number: 1
short_code: "KAIROS-A-0003"
created_at: 2026-03-04T01:40:34.294898+00:00
updated_at: 2026-07-08T15:00:14.504198+00:00
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

# ADR-3: Template System with Reusable Typed Metadata

## Context

Kairos needs two related capabilities:

1. **Templates**: Supporting documents (PRDs, System Context, Architecture Framing, Team Charters, etc.) need starter content so teams aren't staring at a blank page. Templates provide the initial markdown structure.

2. **Structured metadata on items**: All entities need structured data beyond their typed table columns - things like priority, component, repo, stakeholders. The predecessor system (Metis) used YAML frontmatter embedded in markdown content, mixing structured data with free-form content. This creates parsing complexity and makes querying by metadata fields require content parsing.

Key constraints:
- Content is pure markdown. No frontmatter. No structured headers the system parses. Content is content, metadata is metadata.
- Metadata fields must be typed (string, enum, date) so the UI knows how to render them and the API can validate values.
- Metadata definitions should be reusable across templates - a "priority" field defined once can appear on PRD templates, task templates, etc.
- Templates come with a set of metadata fields and optional default values.
- The system ships with useful defaults; tenants can customize.

## Decision

**Three separate concerns: templates (starter content), metadata definitions (reusable typed fields), and item metadata (values on entities).**

### Schema

```
templates
  - id                (uuid)
  - name              (text)
  - slug              (text)
  - content           (text - starter markdown)
  - is_system_default (boolean)
  - created_at        (timestamp)
  - updated_at        (timestamp)

metadata_definitions
  - id                (uuid)
  - name              (text - "priority", "component", "repo")
  - slug              (text)
  - field_type        (string|enum|date)
  - is_system_default (boolean)
  - created_at        (timestamp)
  - updated_at        (timestamp)

metadata_enum_options
  - id                     (uuid)
  - metadata_definition_id (uuid, FK -> metadata_definitions)
  - value                  (text - "low", "medium", "high", "critical")
  - position               (integer, display ordering)

template_metadata
  - id                     (uuid)
  - template_id            (uuid, FK -> templates)
  - metadata_definition_id (uuid, FK -> metadata_definitions)
  - default_value          (text, nullable)
  - required               (boolean)

item_metadata
  - id                     (uuid)
  - item_id                (uuid - references any entity via shared UUID space)
  - metadata_definition_id (uuid, FK -> metadata_definitions)
  - value                  (text)
```

### How It Works

**Metadata definitions** are reusable components defined at the tenant level (or system level for defaults). Define "priority" once with type `enum` and options (low, medium, high, critical). Define "repo" once with type `string`. Define "due_date" once with type `date`.

**Templates** reference metadata definitions through `template_metadata`. A PRD template might include: document_type (enum, default "PRD"), status (enum, default "draft"), stakeholders (string, required). When a document is created from a template:
1. Template's markdown content is copied into the document's `content` field. No ongoing relationship.
2. `item_metadata` rows are created for each `template_metadata` entry, using default values where defined. No pre-population beyond defaults.

**Item metadata** applies to any entity (strategies, initiatives, tasks, documents, ADRs) via the shared UUID space. Items can also have metadata that wasn't defined by a template - ad-hoc metadata added after creation.

**Validation**: The application validates `item_metadata.value` against the referenced `metadata_definition`:
- `string`: any text
- `enum`: value must exist in `metadata_enum_options` for that definition
- `date`: value must be a valid date

### System Defaults

Kairos ships with default metadata definitions:
- priority (enum: low, medium, high, critical)
- status (enum: draft, review, approved)
- complexity (enum: xs, s, m, l, xl)
- document_type (enum: prd, system_context, architecture, charter, social_contract, vision)

And default templates:
- PRD (content + document_type=prd, status=draft)
- System Context (content + document_type=system_context, status=draft)
- Architecture Framing (content + document_type=architecture, status=draft)
- Team Charter (content + document_type=charter)
- Social Contract (content + document_type=social_contract)
- Company Vision (content + document_type=vision)

Tenants can add their own metadata definitions and templates.

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **A: Templates as markdown only, no metadata system** | Simplest, no additional tables | No structured querying by fields, back to parsing content for data, JIRA-like custom fields impossible | Low | Low |
| **B: Per-template metadata definitions** | Self-contained templates, no shared definitions | Duplication (priority defined N times), inconsistency across templates, can't query by field across types | Low | Medium |
| **C: Reusable metadata definitions + template association** (chosen) | Define once use everywhere, consistent across templates, queryable, typed | More tables (5 for the whole system), indirection through definition references | Low | Medium |
| **D: JSONB metadata column on entities** | Flexible, no extra tables | Untyped, no validation, no enum support, violates no-JSONB constraint | Medium | Low |

## Rationale

1. **Content and metadata are fundamentally different things.** Mixing them (frontmatter) creates parsing complexity and makes querying expensive. Clean separation means content is always just markdown, metadata is always just typed key-value pairs.

2. **Metadata definitions are reusable.** "Priority" means the same thing whether it's on a task, a PRD, or an initiative. Defining it once ensures consistency and enables cross-type queries ("show me all critical items").

3. **Templates compose content + metadata.** A template isn't just starter text - it also declares what structured data this type of document carries. This is what makes "create a PRD" meaningful - you get the right content structure AND the right metadata fields.

4. **Typed fields enable proper UI rendering.** The API tells the client "this is an enum with these options" or "this is a date" - the client doesn't need to guess. No JSONB, no untyped bags.

5. **No pre-population beyond defaults.** Keeps creation simple. Defaults provide a starting point; teams fill in the rest.

## Consequences

### Positive
- Clean separation of content (markdown) and metadata (typed key-value)
- Reusable metadata definitions prevent duplication and ensure consistency
- Typed fields enable proper UI rendering (dropdowns for enums, date pickers for dates)
- Any entity can have metadata - not just documents
- Templates provide a complete starting point (content + metadata fields)
- System defaults reduce setup; tenants customize as needed

### Negative
- Five tables for the metadata/template system (templates, metadata_definitions, metadata_enum_options, template_metadata, item_metadata)
- Querying "all PRDs" requires joining through item_metadata rather than a type column
- Metadata validation is application-side (enum value checking, date parsing)
- Ad-hoc metadata (not from a template) has no type enforcement unless a definition is created first

### Neutral
- This replaces YAML frontmatter entirely - a departure from Metis's document format
- Templates have no ongoing relationship to documents after creation - template changes don't affect existing documents