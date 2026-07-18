-- Promote board_member_capabilities' UNIQUE constraint to the PRIMARY KEY
-- (KAIROS-T-0009). S-0004 declared UNIQUE (board_id, user_id, capability)
-- with all three columns NOT NULL but no PRIMARY KEY; Diesel requires every
-- mapped table to have a primary key (`diesel print-schema` hard-errors on
-- PK-less tables). The natural key is the same three columns, so this is a
-- semantically equivalent constraint upgrade, not a redesign.
ALTER TABLE board_member_capabilities
    DROP CONSTRAINT board_member_capabilities_board_id_user_id_capability_key;
ALTER TABLE board_member_capabilities
    ADD PRIMARY KEY (board_id, user_id, capability);
