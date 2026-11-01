---
title: "Short codes"
---

A short code identifies one item. Each API, each CLI command and each MCP tool
accepts it. This page gives the rules of short codes.

## Format

A short code has the form `{PREFIX}-{LETTER}-{NUMBER}`, for example
`SKADI-T-0577`.

| Part | Rule |
|---|---|
| `PREFIX` | The code prefix of the board of the item. See [The prefix of a board](#the-prefix-of-a-board). |
| `LETTER` | The type of the item: `S` strategy, `I` initiative, `T` task, `D` document, `A` ADR. |
| `NUMBER` | 4 digits at least, for example `0001`. A number above 9999 has more digits. |

Kairos changes a short code in input to capital letters before the lookup. So
`skadi-t-0577` finds `SKADI-T-0577`. The answer has the code in capitals.

## The prefix of a board

Each board has a code prefix, `code_prefix`.

- An organization admin sets the prefix when the admin creates the board. The
  prefix does not change later.
- The prefix has 2 to 10 characters. The first character is a capital letter.
  Each other character is a capital letter or a digit. The rule is
  `^[A-Z][A-Z0-9]{1,9}$`.
- A board create with no prefix, or with a prefix that does not agree with the
  rule, gets 422 `VALIDATION` with `details.field` = `code_prefix`.
- A request that changes the prefix of a board gets 422
  `CODE_PREFIX_IS_FIXED`.

The board of an item gives its prefix:

| Item | The board that gives the prefix |
|---|---|
| Task | Its delivery board. |
| Initiative | Its initiative board. |
| ADR | Its ADR board. |
| Strategy | Its strategy board. |
| Document | Its owner board. Each document has an owner board, so a document always gets the prefix of a board. |
| An ADR with no board | The prefix of the organization: the organization slug in capitals, with letters and digits only. |

### Two boards with one prefix

Two boards can have one prefix when they hold different types. The level of a
board gives its types:

| Level | Types |
|---|---|
| strategy | `S` |
| initiative | `I` |
| delivery | `T`, `D` |
| ADR | `A` |

Two live boards of the same level cannot have the same prefix. A create with
such a prefix gets 409 `CONFLICT`. The refusal names the board that has the
prefix. Thus a short code alone always finds one item.

Example: the boards `initiatives`, `adrs`, `strategy` and
`colliery-io-delivery` all have the prefix `COLLIERY`. The board `skadi` has
the prefix `SKADI`.

### The ADR board of a team

A team can have one ADR board for its delivery ADRs. That board has the prefix
of the team: the prefix of the delivery board of the team. An ADR on it gets a
code such as `SKADI-A-0001`. See [Set up a board](../../how-to/set-up-a-board/).

## Numbers

Kairos keeps one sequence for each pair of prefix and type. The next item of
that pair gets the next number of the sequence.

- A new prefix starts at 1.
- Two boards that share a prefix for documents also share the sequence of
  `D`. Their codes stay unique.
- A create that fails does not use a number.
- Kairos does not give a number 2 times. A number that an archived item has
  stays in use.

## A rename on a move

A move to a different board keeps the code. A move with `rename` gives the
item the next code of the new board. Kairos then retires the old code, and
each reference to the old code changes one time. See
[Move work between boards](../../how-to/move-work-between-boards/#give-it-a-code-of-the-new-board).

## Retired codes

A retired code is a code that an item had before a rename.

- Kairos does not give a retired code again.
- A read with a retired code finds the item. The answer names the current
  code. `get_item`, search and the item page in the GUI do this.
- A write to the item of a retired code gets 404. The refusal names the
  current code.
- A write that refers to another item follows a retired code, as a read
  does. The answer names the current code. These references follow:
  - the two ends of an edge: `link_items`, `unlink_items` and
    `POST /api/relationships`;
  - the parent of a new item: `create_item` with `parent`, and
    `POST /api/documents` with `parent_short_code`.

The MCP answer has a line that names the current code, for example
`COLLIERY-T-2430 is now SKADI-T-0577.`

The REST answer of `POST /api/relationships` has the current codes in
`source_short_code` and `target_short_code`.

See [Errors](../errors/#a-retired-short-code).

## Archived items

A write to an archived item gets a refusal. The refusal says that the item is
archived. It tells you to restore the item with `restore_item`. The item of
the write gets 404 `NOT_FOUND`. A reference to an archived item, for example
the target of an edge, gets 422 `VALIDATION`.

## Related pages

- [Set up a board](../../how-to/set-up-a-board/)
- [Move work between boards](../../how-to/move-work-between-boards/)
- [Errors](../errors/)
- [Glossary: short code](../glossary/#short-code)
