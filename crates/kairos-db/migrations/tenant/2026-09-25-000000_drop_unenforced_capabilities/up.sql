-- KAIROS-T-0182: remove grants of two capabilities that authorised nothing.
--
-- `configure_templates` and `configure_metadata` were in the A-0006 grantable
-- vocabulary and consulted by no handler, ever. They are now out of the
-- vocabulary, because they cannot be enforced as written: a grant is keyed
-- (board_id, user_id, capability), and `templates` and `metadata_definitions`
-- have no board_id — they are tenant-wide. A per-board grant over a tenant-wide
-- resource is org-admin authority in disguise.
--
-- Deleting these rows changes no access decision: nothing consulted them, so
-- every holder already had exactly the access they have after this runs. What it
-- does change is that `whoami` and the admin interface stop reporting a
-- capability that is no longer in the vocabulary.
--
-- `configure_*` glob grants are deliberately NOT touched. That glob survives,
-- still covers `configure_boards`, and revoking it would remove access someone
-- actually has.
DELETE FROM board_member_capabilities
 WHERE capability IN ('configure_templates', 'configure_metadata');
