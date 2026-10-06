-- A refresh session is a FAMILY of tokens, each rotation adding the next one.
-- `successor_jti` names the token a rotation minted, so a straggler replaying
-- the rotated one within the grace window gets that very successor back
-- instead of a new session; a replay after the grace window, a logout or a
-- device revoke ends the whole family. Each existing token is its own family.
ALTER TABLE refresh_tokens ADD COLUMN family_id BLOB;
ALTER TABLE refresh_tokens ADD COLUMN successor_jti BLOB;
UPDATE refresh_tokens SET family_id = jti;
CREATE INDEX refresh_tokens_family_idx ON refresh_tokens(family_id);
