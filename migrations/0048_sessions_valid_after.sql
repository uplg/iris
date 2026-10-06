-- Every token issued before this instant is dead (access and refresh alike):
-- stamped when the password changes or is reset, so a session opened under
-- the old password can't outlive it until its own expiry. NULL = never.
ALTER TABLE users ADD COLUMN sessions_valid_after TIMESTAMP;
