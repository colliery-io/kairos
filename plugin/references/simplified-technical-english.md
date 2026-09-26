# Simplified Technical English Spec

Source of truth for how Kairos writes **procedural** text. Based on ASD-STE100, Simplified Technical English, issued by the AeroSpace, Security and Defence Industries Association of Europe. Every rule below has an ID, and a reviewer or a checker must be able to cite that ID and point at the text that satisfies or violates it. Rendered from KAIROS-S-0009 (source of truth) — do not edit here.

## 1. What this is for, and what it is not for

STE is a controlled language written for aircraft maintenance procedures. It is very
good at instructions and actively hostile to argument. A vocabulary of roughly 900
words cannot say *"this amends an absolute into a default"*, and it should not try.

So the scope follows the Diátaxis split this repository already uses
([[KAIROS-S-0008]]).

### 1.1 Where STE applies

| Surface | Why |
|---|---|
| `docs/src/tutorials/` | The reader is following steps. |
| `docs/src/how-to/` | The reader is following steps. |
| `docs/src/reference/` | The reader is looking one fact up. |
| Acceptance criteria | Each one is a testable instruction. |
| Error messages | The reader is stuck and reads under stress. |
| CLI help text | The reader wants the flag, not prose. |

### 1.2 Where STE does not apply

| Surface | Why |
|---|---|
| `docs/src/explanation/` | It argues a position. STE cannot. |
| ADRs | They record why a decision beat the alternatives. |
| Task Status Updates | They record reasoning, surprises and rejected options. |
| Commit messages | They explain a change to a future reader. |
| Code comments | They explain why the code is shaped this way. |

**The exclusions are the point, not an escape hatch.** The sentences an ADR exists to
carry are the ones a controlled vocabulary cannot express. Constraining them would
flatten the reasoning this repository keeps on purpose. If a writer finds STE fighting
them, the first question is whether the text is procedural at all — and if it is not,
STE does not apply to it.

## 2. The approved vocabulary, and why it is not in this repository

ASD-STE100 has been **free of charge since Issue 6 (2013)**, and its copyright is
**fully owned by ASD**. Copies are distributed by the STE Maintenance Group on
request, not redistributed by third parties.

**Kairos therefore ships no copy of the dictionary.** This specification carries the
writing rules, which are describable in our own words, and the two lists that are ours
to maintain. A writer who wants the approved-word list requests the specification from
<https://www.asd-ste100.org>.

The consequence is stated rather than hidden: **rule STE-V1 below cannot be checked
mechanically in this repository.** Section 6 says what is checked and what is not.

## 3. The writing rules

Each rule has an ID. Cite it.

### Sentences

| ID | Rule |
|---|---|
| STE-S1 | Write one instruction in one sentence. Two actions are two sentences. |
| STE-S2 | Keep a procedural sentence to 20 words or fewer. |
| STE-S3 | Keep a descriptive sentence to 25 words or fewer. |
| STE-S4 | Keep a paragraph to 6 sentences or fewer. |

A step that needs a reason gets two sentences: the instruction, then the reason. Do not
join them with "because" into one long sentence.

### Grammar

| ID | Rule |
|---|---|
| STE-G1 | Use the active voice. Name who does the thing. |
| STE-G2 | Use the present tense. |
| STE-G3 | Use the imperative for an instruction. "Set the variable", not "the variable should be set". |
| STE-G4 | Do not chain gerunds. "Starting the server after setting the variable" becomes two sentences. |
| STE-G5 | Never drop an article. "Set the variable", not "Set variable". |
| STE-G6 | Do not write a noun cluster of more than three words. |

### Words

| ID | Rule |
|---|---|
| STE-V1 | Use a word from the approved vocabulary, in its approved meaning only. Replace an unapproved word; do not explain it in parentheses. |
| STE-V2 | Use one term for one concept, always. A synonym for variety is a defect. |
| STE-V3 | Use a Technical Name or Technical Verb from section 4 for a domain term. |
| STE-V4 | Do not use an abbreviation before you have written it out once on the page. |

STE-V2 matters more here than it looks. "Organization" and "tenant" are the same thing
in Kairos, and using them interchangeably on one page teaches the reader that they are
different. Section 4 fixes which word wins.

### Procedures

| ID | Rule |
|---|---|
| STE-P1 | Put a condition before the instruction. "If the login fails, read the log", not "read the log if the login fails". |
| STE-P2 | Give a warning before the step it applies to, never after. |
| STE-P3 | Number the steps when their order matters. Use a list when it does not. |

## 4. Technical Names and Technical Verbs

Every Kairos domain term is a Technical Name under STE, and none of them are in the
approved dictionary. This list is the part that makes STE-V3 real instead of
aspirational. **Maintaining it is part of adding a domain concept.**

### 4.1 Technical Names

| Approved | Do not write | Note |
|---|---|---|
| organization | tenant, org, company, workspace | "Tenant" is the schema; "organization" is what a reader has. Use "tenant" only when writing about the schema itself. |
| board | kanban, wall, swimlane | |
| column | state, status, bucket | A column is the place; a transition moves between them. |
| lane | row, track | |
| strategy | — | A work item type. Never lower-case as a general noun in procedural text. |
| initiative | project, epic | |
| task | ticket, issue, story, card | "Card" is the GUI element, not the work item. |
| document | doc, page, note | |
| ADR | decision record | Write "architecture decision record (ADR)" once per page first (STE-V4). |
| short code | id, code, reference, key | The `KAIROS-T-0001` form. |
| capability | permission, right, role | Kairos has no roles on boards. See KAIROS-A-0006. |
| delivery stream | stream, value stream | |
| team | squad, group | |
| proposal | suggestion, recommendation | A retrieval result a person confirms. |
| service account | bot, machine user, robot account | |
| API key | token, secret | A `kairos_sk_…` credential. |
| session | login, sign-in state | A `kairos_ss_…` bearer from a password login. |
| issuer | IdP, identity provider, provider | Use "issuer" for the OIDC endpoint. "Identity provider" is allowed in explanation text, which is out of scope. |
| claim | attribute, field | A field inside a token. |
| deployment | install, installation, instance | |
| organization admin | org admin, administrator | |
| deployment admin | superuser, root, sysadmin | |

### 4.2 Technical Verbs

Permitted beyond the approved list, in these meanings only.

| Verb | Meaning |
|---|---|
| provision | Create an organization and its schema. |
| transition | Move a work item from one column to another. |
| archive | Put work away without deleting it. See KAIROS-A-0004. |
| revoke | Make a credential stop working, and keep the record. |
| grant | Give a capability on a board. |
| embed | Compute a vector for text. |
| propose | Offer a result for a person to confirm. |
| confirm | Accept a proposal. |
| throttle | Refuse a request because too many failed. |
| seed | Write fixture data into a deployment. |

Do **not** invent a Technical Verb in a page. Add it here first, with its meaning, or
use an approved word.

## 5. Worked examples

Each pair is a real edit, not an invented one.

**STE-S2, STE-G1, STE-G4.** Before:

> Having configured the issuer, the deployment should be restarted so that the new
> settings are picked up by the server on boot.

After:

> Restart the deployment. The server reads the new settings when it starts.

**STE-P1.** Before:

> Read the log to find out whether the bootstrap admin was created if the login fails.

After:

> If the login fails, read the log. The log says whether Kairos created the bootstrap
> admin.

**STE-V2.** Before:

> An org admin adds the user to the workspace. The administrator can then grant the
> member permissions on the team's wall.

After:

> An organization admin adds the person to the organization. The organization admin
> then grants capabilities on the team's board.

**STE-G5, STE-G6.** Before:

> Set bootstrap admin password hash environment variable value.

After:

> Set the `KAIROS_BOOTSTRAP_PASSWORD_HASH` variable.

## 6. How compliance is checked, and what is not checked

An unenforced style rule decays, and a rule everyone believes is enforced while it is
not is worse than an acknowledged aspiration. So this section is explicit.

### 6.1 Checked mechanically — `angreal docs ste`

The checker reads `docs/src/{tutorials,how-to,reference}` and reports these rules:

| ID | How |
|---|---|
| STE-S2 | Sentence word count over the limit. |
| STE-S4 | Paragraph sentence count over the limit. |
| STE-G1 | Passive-voice construction: a form of "to be" followed by a past participle. |
| STE-G4 | Two or more gerunds in one sentence. |
| STE-V2, STE-V3 | A word in the "do not write" column of section 4.1. |

It runs in CI. **It is baselined, not big-bang**: the repository has a committed
count of current violations per file, and the gate fails when a file gets *worse*. A
new file starts at zero. That is how the rule applies from today forward without a
rewrite nobody reviewed — and the baseline only ever goes down.

### 6.2 Checked by a reviewer

STE-S1, STE-S3, STE-G2, STE-G3, STE-G5, STE-G6, STE-V4, STE-P1, STE-P2 and STE-P3.
A reviewer cites the rule ID.

### 6.3 Not checked at all

**STE-V1, the approved vocabulary.** It needs the ASD word list as data, and section 2
explains why this repository does not carry it. This is the honest gap: full STE
conformance is aspirational here, and a writer who wants it downloads the
specification. Everything else on this page is either mechanical or reviewable.

### 6.4 Not in scope of any check

`docs/src/explanation/`, ADRs, Status Updates, commit messages and code comments —
section 1.2. The checker does not read them. If it ever starts to, that is a defect in
the checker.

## 7. Review triggers

- A new domain concept arrives and section 4 does not name it.
- The baseline in 6.1 stops going down over several releases, which means the rule is
  being worked around rather than followed.
- ASD changes the licensing position on the approved vocabulary, which would let
  STE-V1 be checked.