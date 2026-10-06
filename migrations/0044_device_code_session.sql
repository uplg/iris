-- The session a claimed pairing code handed out, so a repeated poll (a lost
-- response, a retry) replaces it instead of minting another device row.
ALTER TABLE device_codes ADD COLUMN session_jti BLOB;
