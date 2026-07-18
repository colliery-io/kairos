-- Restore the original S-0004 shape: UNIQUE constraint, no PRIMARY KEY.
ALTER TABLE board_member_capabilities
    DROP CONSTRAINT board_member_capabilities_pkey;
ALTER TABLE board_member_capabilities
    ADD CONSTRAINT board_member_capabilities_board_id_user_id_capability_key
    UNIQUE (board_id, user_id, capability);
