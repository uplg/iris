-- Passkeys: an optional second way in, next to the password. One row per
-- credential; the user handle the authenticator stores is the user's id,
-- so a discoverable sign-in names its owner.
CREATE TABLE webauthn_credentials (
    id               BLOB      PRIMARY KEY NOT NULL,
    user_id          BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- base64url, as webauthn-rs writes it
    credential_id    TEXT      NOT NULL UNIQUE,
    -- the serialised webauthn-rs Passkey (public key, counter, flags)
    passkey_json     TEXT      NOT NULL,
    -- what the user calls it; first named after the device it was made on
    name             TEXT      NOT NULL DEFAULT '',
    backup_eligible  BOOLEAN   NOT NULL DEFAULT FALSE,
    backed_up        BOOLEAN   NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMP NOT NULL,
    last_used_at     TIMESTAMP
);

CREATE INDEX webauthn_credentials_user_idx ON webauthn_credentials(user_id);
