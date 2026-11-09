---
name: grilling
description: Grill the user relentlessly about a plan or design in the conversation. Use when the user wants to stress-test a plan before building, or says "grill me". This loop writes nothing to Kairos. To grill a Kairos initiative, a strategy or the tasks of an initiative, the user runs /kairos:grill-initiative or /kairos:grill-decomposition.
---

Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

Ask the questions one at a time, waiting for feedback on each question before continuing. Asking multiple questions at once is bewildering.

Before the first question, ground the plan in the record: [GROUND.md](../kairos/GROUND.md) gives the calls (the ADRs and documents of the repository, the documents of the initiative, `related_work`) and the sentence to write ("Read: ... Nothing read contradicts this plan."). Name what you read. A plan step that contradicts an ADR says so.

If a *fact* can be found by exploring the codebase, look it up rather than asking me. The *decisions*, though, are mine — put each one to me and wait for my answer.

Code index first. Find code with the tools of the `kairos-code` MCP server before you read a file. `code_search` finds what the code does; `symbol` finds one name. `callers` and `references` find its uses; `path` finds how one symbol reaches another. Then read only the files that they name. grep is for literal text only: a log line, a config key, an error message. After you change code, run `kairos index update`.

Do not enact the plan until I confirm we have reached a shared understanding.

This loop writes nothing to Kairos. `/kairos:grill-initiative` and `/kairos:grill-decomposition` run it and write each answer into the item. If the user asks to grill a Kairos item and neither skill runs, offer the one that fits.

Name work the way Kairos does: an existing item by its short code, a proposed item by its quoted title marked "not yet created", and no numbered decisions (`D1`, `D2`). The rules are in the `kairos-vocabulary` skill.
