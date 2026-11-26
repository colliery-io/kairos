---
name: to-initiative
description: Turn the current conversation into a Kairos initiative with an attached PRD — no interview, just synthesis of what's already been discussed.
disable-model-invocation: true
---

Everything this skill needs is already in the conversation — stress-testing a half-formed plan is `/kairos:grill-me`, run before this skill, not during it.

## Process

1. Explore the repo, if you haven't already, until you can name the modules this feature touches and the candidate seams. Use the project's domain glossary vocabulary throughout the PRD, and respect ADRs in the area you're touching — `search` Kairos for both if they aren't already in context.

   Code index first. Find code with the tools of the `kairos-code` MCP server before you read a file. `code_search` finds what the code does; `symbol` finds one name. `callers` and `references` find its uses; `path` finds how one symbol reaches another. Then read only the files that they name. grep is for literal text only: a log line, a config key, an error message. After you change code, run `kairos index update`.

2. Sketch out the seams at which you're going to test the feature. Existing seams should be preferred to new ones. Use the highest seam possible. If new seams are needed, propose them at the highest point you can. The fewer seams across the codebase, the better — the ideal number is one.

   Check with the user that these seams match their expectations. The confirmed seams become the backbone of the PRD's Testing Decisions.

3. **Keep the initiative to one objective** (KAIROS-T-0780). Before you create it, check the scope with the user: one objective, and the work that delivers it. Work for a different objective goes to its own initiative. Name a dependency on another initiative at the start, as a `blocks` edge between the two initiatives. Kairos gives the same guidance in the answer of `create_item`, and warns when a `blocks` edge joins tasks of two initiatives.

4. Publish to Kairos:

   - The board of the initiative is `initiative_board` of the wiring (the SessionStart context, or `kairos config show`). With no value there, find it with `my_boards`. If more than one board can hold this work, or none exists, ask the user.
   - `create_item(item_type: initiative, board: <initiative board>, title: ...)` — a title in the domain glossary's words. The create takes no content: it writes the template of the type as version 1 and gives it back. Fill it in with `update_item` (send version 1). Write the problem and the intended outcome in its Context and its Goals & Non-Goals.
   - `create_item(item_type: document, template: prd, board: <team board>, parent: <initiative short code>, title: ...)`. The create takes no content: it writes the template of the type as version 1 and gives it back. Fill it in with `update_item` (send version 1). `board` is the owner board of the PRD (required); the `parent` edge attaches the PRD to the initiative — no other wiring needed. The PRD is done when every decision made in the conversation lands in a section of the template — requirements, non-functional requirements, or Out of scope; nothing decided in-chat is left implicit.

   Name the initiative and the PRD only by the short codes in the results of `create_item`. Work for later is a quoted title, not "phase 2" (the `kairos-vocabulary` skill).

   Finish by telling the user both short codes. Breaking the initiative into tasks is `/kairos:decompose`, when the user is ready.

