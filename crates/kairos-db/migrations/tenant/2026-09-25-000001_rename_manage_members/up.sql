-- KAIROS-T-0183: `manage_members` becomes `administer_members`.
--
-- Capability matching is a textual LIKE translation (KAIROS-A-0006), so while
-- board administration was named `manage_members` the `manage_*` glob covered
-- it. An admin granting `manage_*` as shorthand for "the work-item
-- capabilities" also handed over the power to grant and revoke other people's
-- capabilities. Renaming it out of the prefix makes `manage_*` a real family
-- glob without putting a second rule in the matcher — which A-0006 requires the
-- pure matcher and the SQL check to share exactly.
--
-- Explicit grants carry over unchanged in meaning:
UPDATE board_member_capabilities
   SET capability = 'administer_members'
 WHERE capability = 'manage_members';

-- BREAKING, and deliberately not papered over: a holder of `manage_*` LOSES
-- board administration here, because that is the defect. This migration does
-- NOT grant them `administer_members` to compensate.
--
-- Doing so would preserve exactly the access the ticket exists to remove, and it
-- cannot distinguish the admin who typed `manage_*` meaning "all the manage
-- capabilities" from one who meant "and board administration too" — the whole
-- problem is that the vocabulary never let them say which.
--
-- An operator who did intend it grants `administer_members` explicitly, which is
-- now a thing they can say. To find who is affected before upgrading:
--
--   SELECT board_id, user_id FROM board_member_capabilities
--    WHERE capability IN ('manage_*', '*');
--
-- (`*` holders are unaffected — it matches everything, including the new name.)
