-- Reverses the rename. Grants made under the new name become `manage_members`
-- again, which restores the A-0006 textual-matching behaviour this migration
-- exists to fix: `manage_*` will cover them once more.
UPDATE board_member_capabilities
   SET capability = 'manage_members'
 WHERE capability = 'administer_members';
