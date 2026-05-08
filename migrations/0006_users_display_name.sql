-- Public-facing display name on the user record. The email stays private —
-- exposed only on the user's own /me payload and to admins. Everywhere else
-- (Library "added by", future comments, ...) we surface the display name.
--
-- Backfill rule (mirrored in `iris_db::users::default_display_name`):
-- take the email's local-part, then truncate at the first dot.
--   leonard.apollo@uplg.xyz → leonard
--   johndoe@example.com           → johndoe
-- Easy to override later from /account.
ALTER TABLE users ADD COLUMN display_name TEXT;

UPDATE users SET display_name =
  CASE
    -- local-part contains a dot: take everything before that dot.
    WHEN instr(substr(email, 1, instr(email, '@') - 1), '.') > 0
      THEN substr(email, 1, instr(substr(email, 1, instr(email, '@') - 1), '.') - 1)
    -- otherwise just the local-part.
    WHEN instr(email, '@') > 1
      THEN substr(email, 1, instr(email, '@') - 1)
    -- pathological no-@ email: keep as is.
    ELSE email
  END
WHERE display_name IS NULL OR display_name = '';
