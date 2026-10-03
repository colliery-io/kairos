# Short codes

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
| Document | Its owner board. A document with no owner board takes the prefix of the organization. |
| An item with no board | The prefix of the organization: the organization slug in capitals, with letters and digits only. |

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
code such as `SKADI-A-0001`. See [Set up a board](../how-to/set-up-a-board.md).

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
[Move work between boards](../how-to/move-work-between-boards.md#give-it-a-code-of-the-new-board).

## Retired codes

A retired code is a code that an item had before a rename.

- Kairos does not give a retired code again.
- A read with a retired code finds the item. The answer names the current
  code. `get_item`, search and the item page in the GUI do this.
- A write with a retired code gets 404. The refusal names the current code.

See [Errors](errors.md#a-retired-short-code).

## Keep the numbers of an import

The Metis importer (`scripts/migrate-metis-to-kairos.py --codes keep`) keeps
the Metis numbers on the boards of a team. `SKADI-T-0577` stays
`SKADI-T-0577`. A Metis specification `SKADI-S-0003` becomes the document
`SKADI-D-0003`.

Before each create, the importer sets the sequence of the board with this
request:

```text
PUT /api/boards/{board}/code-sequences/{item_type}
{"next_number": 577}
```

The next create of that type on the board then gets the number 577.

- Only an organization admin can send the request.
- `item_type` is `strategy`, `initiative`, `task`, `document` or `adr`. The
  board must hold the type. A delivery board holds tasks and documents.
- A sequence does not go back. The number must be above the last number of
  the sequence. It must also be above the number of each code of that prefix
  and type, live, archived or retired.
- The same request 2 times gives the same result.

| Refusal | Why |
|---|---|
| 409 `CODE_IN_USE` | An item has the code. `details.code` names it. |
| 409 `CODE_RETIRED` | The code is retired. `details.current_code` names the current code of its item. |
| 409 `SEQUENCE_IS_PAST` | The sequence is at the number or above it. `details.next_code` names the next code. |
| 422 `VALIDATION` | The board does not hold `item_type` (`details.parameter`), or `next_number` is below 1 (`details.field`). |

The importer stops at the first refusal. Its message names the Metis code, the
code and the reason.

### What the importer does with codes

- An item keeps its Metis number when the board that gives its code has the
  prefix `--prefix`. `--prefix` must be the prefix of the delivery board.
- An initiative on the shared board `initiatives` gets the next code of that
  board. A document that supports a parent gets the next code of the
  organization prefix.
- Two Metis documents can want one code. Examples: a Metis code that 2 files
  have, or the vision `X-V-0001` and the specification `X-S-0001`. The first
  document keeps the code. The other document gets the next free number. Its
  footer says why.
- After each item exists, the importer changes each reference in the text to
  the new code. The footer keeps the Metis code.

The importer changes a code in a path, for example `/FIDIUS-S-0001/`. A rename
on a move does not change a code in a path, a URL or a file name.

## Related pages

- [Set up a board](../how-to/set-up-a-board.md)
- [Move work between boards](../how-to/move-work-between-boards.md)
- [Errors](errors.md)
- [Glossary: short code](glossary.md#short-code)
