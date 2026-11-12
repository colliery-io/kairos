-- KAIROS-T-0764 (COLLIERY-I-0674, KAIROS-A-0084): the AdHoc stand-up. A
-- saved, named group of people of the organization.
--
--   standup_groups        One group: its name (unique in the organization,
--                         with no regard to case) and its creator. Only
--                         the creator deletes it; each member edits it.
--   standup_group_people  The people of a group, one row each.
--
-- No foreign key on created_by and user_id: users are in the public
-- schema. A delete of a group deletes its people (ON DELETE CASCADE).
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 2 new tables, empty. No row is changed.
--
-- Re-runnable: each statement has a guard.
CREATE TABLE IF NOT EXISTS standup_groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 100),
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS standup_groups_name_key ON standup_groups (lower(name));

CREATE TABLE IF NOT EXISTS standup_group_people (
    group_id UUID NOT NULL REFERENCES standup_groups (id) ON DELETE CASCADE,
    user_id  UUID NOT NULL,
    PRIMARY KEY (group_id, user_id)
);
