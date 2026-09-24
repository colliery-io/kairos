-- KAIROS-T-0192 (KAIROS-A-0021 rule 6): an agent proposes a relationship, a
-- human confirms it.
--
-- WHY THIS TABLE EXISTS RATHER THAN LETTING AGENTS WRITE EDGES
--
-- Agents write most of the content in this product and humans edit; that
-- division is its shape. Edges are the exception, because the blast radius is
-- different in kind: a wrong `parent` re-parents work onto a board that then
-- reports the wrong thing to the wrong people, and nobody looks at a parent edge
-- twice once it exists. KAIROS-T-0190 measured the precision available at the
-- top of the similarity distribution at roughly half. A proposal costs a click;
-- a wrong edge costs a conversation.
--
-- REJECTIONS ARE KEPT, NOT DELETED
--
-- A rejected proposal is the only honest measurement this feature has. If a pair
-- keeps being proposed and keeps being rejected, the retrieval is wrong about
-- something and the confirm/reject ratio is where that shows up. Deleting
-- rejections would delete the evidence that the feature is not working.
--
-- `source_id`/`target_id` carry no foreign key, for the same reason
-- `item_relationships` carries none: an item is a row in one of five tables.

CREATE TABLE IF NOT EXISTS edge_proposals (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID NOT NULL,
    target_id       UUID NOT NULL,
    -- Only the two relationships worth proposing. `parent` and `blocks` are the
    -- ones that change what a board reports and what an agent picks up next;
    -- `supports`, `informs` and `supersedes` are editorial and a wrong one is
    -- cheap to undo, so they stay a human's to draw.
    relationship    TEXT NOT NULL CHECK (relationship IN ('parent', 'blocks')),
    state           TEXT NOT NULL DEFAULT 'pending'
                    CHECK (state IN ('pending', 'confirmed', 'rejected')),
    -- The evidence, exactly as the retrieval surface rendered it, so a human
    -- reviewing this sees what the agent saw rather than a summary of it.
    claim           TEXT NOT NULL,
    why             TEXT NOT NULL,
    score           REAL NOT NULL,
    -- Who proposed it (a service account, usually) and who ruled on it.
    proposed_by     UUID NOT NULL,
    decided_by      UUID,
    decided_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (source_id != target_id),
    -- A decision has a decider, and an undecided proposal has neither. Enforced
    -- rather than assumed: a confirmed row with no `decided_by` is unauditable,
    -- and audit is most of the point of keeping these.
    CHECK ((state = 'pending') = (decided_by IS NULL)),
    CHECK ((decided_by IS NULL) = (decided_at IS NULL))
);

-- ONE pending proposal per (pair, relationship). An agent loop that proposes on
-- every run would otherwise bury the signal under its own output; this makes
-- re-proposing an existing pending proposal a no-op at the database level rather
-- than something every caller has to remember.
--
-- Partial, on `pending` only: a rejected proposal must not block a later
-- re-proposal once there is materially better evidence, and a confirmed one is
-- history.
CREATE UNIQUE INDEX IF NOT EXISTS idx_edge_proposals_pending
    ON edge_proposals (source_id, target_id, relationship)
    WHERE state = 'pending';

-- "What is waiting on this item?" — the question the GUI asks on every item it
-- shows, in both directions, because a proposal concerns both ends.
CREATE INDEX IF NOT EXISTS idx_edge_proposals_source ON edge_proposals (source_id, state);
CREATE INDEX IF NOT EXISTS idx_edge_proposals_target ON edge_proposals (target_id, state);
