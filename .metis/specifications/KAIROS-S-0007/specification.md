---
id: architecture-review-spec
level: specification
title: "Architecture Review Spec"
short_code: "KAIROS-S-0007"
created_at: 2026-07-08T11:29:11.571741+00:00
updated_at: 2026-07-08T11:29:11.571741+00:00
parent: KAIROS-I-0002
blocked_by: []
archived: false

tags:
  - "#specification"
  - "#phase/discovery"


exit_criteria_met: false
initiative_id: NULL
---

# Architecture Review Spec

This document is the single source of truth for the `architecture-review` skill. A reviewer (human or AI) applies it verbatim. Every rule below is checkable against the codebase; if a rule cannot be checked, it does not belong here. It renders into the skills plugin as `references/architecture-review.md` (KAIROS-A-0014); this Metis document is the source of truth.

## 1. Purpose and process

An architecture review answers two questions at two altitudes:

- **Altitude 1 (module/code):** Are the modules deep? Do interfaces hide complexity, or leak it to callers?
- **Altitude 2 (system):** Does the implemented structure match the documented structure, and are the load-bearing decisions recorded?

A review runs in three passes:

1. **Scan.** Inventory the modules (Altitude 1) and the containers/components plus any C4 views and ADRs (Altitude 2). Read interfaces before implementations. Read ADRs before forming opinions.
2. **Classify.** Every candidate finding is assigned exactly one altitude. If a finding spans both (e.g., a shallow module that also violates a container boundary), file two findings and cross-reference them.
3. **Report.** Emit findings in the format of Section 4, ordered by recommendation strength, then by altitude. No finding without evidence. No evidence without a file path or document reference.

Do not report style, formatting, naming-convention, or lint-class issues. This review grades structure only.

## 2. Altitude 1 — Module and code level

### 2.1 Vocabulary

Use these terms exactly. Do not substitute "component," "service," or "boundary" at this altitude (C4 terms are reserved for Altitude 2).

- **Module** — anything with an interface and an implementation: a function, class, package, or tier-spanning slice. Scale-agnostic.
- **Interface** — everything a caller must know to use the module correctly: type signature plus invariants, ordering constraints, error modes, required configuration, and performance characteristics. Not just the type-level surface.
- **Implementation** — the module's body of code.
- **Depth** — leverage at the interface: behaviour a caller or test can exercise per unit of interface learned. Deep = lots of behaviour behind a small interface. Shallow = interface nearly as complex as the implementation. Depth is a property of the interface, not the implementation — never measure it as a ratio of implementation lines to interface lines.
- **Seam** — a place where behaviour can be altered without editing in that place; where a module's interface lives. Seam placement is a design decision distinct from what goes behind it.
- **Adapter** — a concrete thing that satisfies an interface at a seam. Describes role, not substance.
- **Leverage** — what callers get from depth: more capability per unit of interface learned.
- **Locality** — what maintainers get from depth: change, bugs, knowledge, and verification concentrate in one place.

### 2.2 Heuristics

- **Interface complexity vs implementation complexity.** For each significant module, compare what a caller must know against what the module does. Flag modules where callers must understand internals (flags that select code paths, ordering requirements, config the caller shouldn't own) to use it correctly.
- **Information hiding / leakage.** A design decision should live inside exactly one module. If changing one decision (a schema, a format, a protocol) forces edits in multiple modules, that decision has leaked. Flag the leak, name the decision, and name the module that should own it.
- **Shallow-module smells.** Pass-through methods (a method that calls another method with the same signature and adds nothing), wrapper classes that re-expose everything they wrap, interfaces with a method per implementation detail, config objects that mirror internal structure.
- **The deletion test.** Imagine deleting the module. If complexity vanishes, it was a pass-through — recommend deletion or merging. If complexity would reappear across N callers, it is earning its keep. Apply this before proposing any new abstraction: would the proposal concentrate complexity, or just move it?
- **Locality of bugs.** In well-factored code, bugs live in how pure functions are called — in the orchestration — not in the pure functions themselves. Flag modules where verifying correctness requires reading many call sites; recommend deepening so verification concentrates behind the interface.
- **Seam discipline.** One adapter means a hypothetical seam; two adapters means a real one. Flag every port/interface with a single implementation and no concrete second adapter (production + test counts as two). A single-adapter seam is indirection, not architecture — recommend inlining it.
- **Leverage check.** For each abstraction, count call sites and tests that pay it back. An abstraction used once has no leverage; flag it.
- **Design-it-twice.** For any Strong finding proposing a new or reshaped interface, sketch at least two materially different interface designs and state why the recommended one wins on depth, locality, and seam placement. Never present the first design as the only option.

### 2.3 Checklist

Answer each item yes/no per module reviewed. Every "no" is a candidate finding.

- [ ] A1-1: The interface is smaller than the implementation in concepts a caller must learn (methods, params, invariants, error modes) — not merely in lines.
- [ ] A1-2: No method is a pass-through: none forwards to another method with the same signature and no added behaviour.
- [ ] A1-3: No design decision (schema, format, protocol, algorithm choice) is known to more than one module.
- [ ] A1-4: The module survives the deletion test: deleting it would scatter real complexity across two or more callers.
- [ ] A1-5: Every seam has at least two adapters (production + test counts); no single-adapter ports exist.
- [ ] A1-6: The module accepts its dependencies rather than constructing them internally.
- [ ] A1-7: Core logic returns results rather than producing side effects; side effects are pushed to the edges.
- [ ] A1-8: Tests exercise the module through its interface only; no test reaches past the interface into internal state. A deliberately designed internal seam whose adapters include a test adapter counts as interface for this item — the violation is tests groping unexposed internals, not tests using a real seam. *(Clause added 2026-07-10 to reconcile with the codebase-design discipline's seam rules.)*
- [ ] A1-9: Callers do not pass flags or config that select between the module's internal code paths.
- [ ] A1-10: Fixing a plausible bug in this module's domain would require editing one module, not several.
- [ ] A1-11: Every abstraction in the module has at least two call sites or a concrete, named second use; none exists "for the future."
- [ ] A1-12: Errors are handled or made impossible inside the module where feasible, rather than defined into the interface for every caller to handle.

## 3. Altitude 2 — System level

### 3.1 Views

Use the C4 model. Code-level (level 4) diagrams are not required and their absence is never a finding.

- **System Context** — the system, its users, and the external systems it talks to.
- **Container** — the separately deployable/runnable units (services, SPAs, databases, queues) and the interactions between them.
- **Component** — the major structural parts inside one container and their responsibilities.

If no C4 views exist, that is itself a finding (A2-1). If views exist, the review's primary job at this altitude is **drift detection**: does the implemented structure match the documented views?

### 3.2 Heuristics

- **Structure–documentation match.** For each documented container and relationship, find its counterpart in the code (deploy configs, service directories, network calls). Flag: documented elements with no implementation, implemented containers/dependencies absent from the views, and relationships whose direction differs from the diagram.
- **Dependency direction.** Dependencies between containers must point in one deliberate direction (typically toward the domain core or downstream services). Flag cycles between containers, and flag lower-level containers that call upward into their consumers.
- **Coupling between containers.** Containers must interact through published interfaces (APIs, events, queues) — never by reaching into each other's internals. Flag shared databases written by two containers, one container importing another's internal code, and containers coupled to each other's message internals rather than a documented contract.
- **Data ownership.** Every persistent datum has exactly one owning container; all other containers access it through the owner's interface. Flag any table, bucket, or topic written by more than one container, and any consumer that reads another container's store directly.
- **ADR discipline.** Every load-bearing decision — one that would be expensive to reverse or that constrains other decisions (datastore choice, sync vs async integration, authn/authz model, container split) — must have an ADR. Flag load-bearing structures with no recorded decision.
- **No silent re-litigation.** If code has moved away from what an accepted ADR decided, the decision was re-litigated silently. Flag the contradiction; the fix is either a superseding ADR or reverting the code — the review recommends which, it does not decide by fiat.
- **Superseded chains intact.** Every superseded ADR must link forward to its successor, and the successor must reference what it supersedes. No two accepted ADRs may contradict each other.

### 3.3 Checklist

- [ ] A2-1: System Context and Container views exist and were updated within the life of the current structure (no container added/removed since the last diagram update).
- [ ] A2-2: Every implemented container and inter-container dependency appears in the Container view, and vice versa; arrow directions match reality.
- [ ] A2-3: No dependency cycles exist between containers.
- [ ] A2-4: No container reads or writes another container's datastore directly; every persistent store has exactly one writer-owner.
- [ ] A2-5: Inter-container interaction goes through published interfaces; no container imports another container's internal code.
- [ ] A2-6: Every load-bearing decision visible in the structure has an ADR; list any that do not.
- [ ] A2-7: No accepted ADR is contradicted by the current code; contradictions are flagged with both the ADR ID and the code location.
- [ ] A2-8: All superseded ADRs link to their successors; no two accepted ADRs conflict.

## 4. Finding format

Every finding uses exactly this structure:

```
### [ALTITUDE-1|ALTITUDE-2] <short title>
- Checklist item: A1-n / A2-n (or "heuristic: <name>" if no item applies)
- Location: <file path(s), module name, container name, or ADR ID>
- Problem: <one sentence — what structural property is violated>
- Evidence: <concrete observations: the pass-through signature, the leaked
  decision and the N files that know it, the diagram element vs the code,
  the ADR text vs the implementation. Quote or cite; never assert bare.>
- Proposed deepening/restructure: <the specific change — merge X into Y,
  move the seam to Z, define a port with production + test adapters,
  write a superseding ADR, redraw the Container view. For Strong interface
  proposals, include the design-it-twice alternative and why it loses.>
- Strength: Strong | Worth exploring | Speculative
```

Strength calibration:

- **Strong** — evidence is direct and the fix is well-understood; the reviewer would make this change themselves. Requires passing the deletion test and, for new interfaces, design-it-twice.
- **Worth exploring** — the smell is real but the fix has trade-offs or needs input from the owners.
- **Speculative** — pattern suggests a problem but evidence is circumstantial; state what evidence would confirm it.

A review with zero findings must say so explicitly and list the checklist items verified.

## 5. Anti-patterns for the reviewer

Reviews exhibiting these are defective. Self-check before reporting.

- **Grading style, not structure.** Naming, formatting, comment density, and idiom preferences are out of scope. If a finding would be fixed by a linter or a rename, delete it.
- **Re-litigating accepted ADRs.** Proposing a rewrite that an accepted ADR already decided against is not a finding — unless new evidence exists, in which case the finding targets the ADR (propose superseding it), never the code alone.
- **Counting abstractions as depth.** More interfaces, more layers, and more indirection are not depth. Depth is leverage at the interface. A proposal that adds a seam without two justified adapters is itself a shallow-module smell.
- **Moving complexity and calling it removal.** Apply the deletion test to your own proposals: if the recommended refactor relocates complexity into callers or a new pass-through layer, it fails.
- **Testing past the interface.** Recommending tests of internal state or private seams. The interface is the test surface; tests should survive internal refactors.
- **Diagram worship.** Treating the C4 views as authoritative when the code disagrees. Drift is the finding; either artifact may be the one to change.
- **Findings without evidence.** Any problem statement not backed by a file path, quoted signature, or document ID is an opinion, not a finding. Cut it.
- **One-design proposals.** Presenting a single interface redesign as inevitable. Strong interface findings must show design-it-twice was applied.
- **Altitude mixing.** Filing a container-coupling problem as a module smell (or vice versa) buries it. One finding, one altitude, cross-reference when both apply.