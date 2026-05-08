-- Iris initial schema
CREATE TABLE users (
    id              BLOB PRIMARY KEY NOT NULL,
    email           TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    is_admin        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMP NOT NULL
);

CREATE INDEX users_email_idx ON users(email);

CREATE TABLE invitations (
    id              BLOB PRIMARY KEY NOT NULL,
    token_hash      TEXT NOT NULL UNIQUE,
    created_by      BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TIMESTAMP NOT NULL,
    expires_at      TIMESTAMP NOT NULL,
    consumed_at     TIMESTAMP,
    consumed_by     BLOB REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX invitations_token_hash_idx ON invitations(token_hash);
CREATE INDEX invitations_active_idx
    ON invitations(expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE refresh_tokens (
    jti             BLOB PRIMARY KEY NOT NULL,
    user_id         BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issued_at       TIMESTAMP NOT NULL,
    expires_at      TIMESTAMP NOT NULL,
    revoked_at      TIMESTAMP
);

CREATE INDEX refresh_tokens_user_idx ON refresh_tokens(user_id);
CREATE INDEX refresh_tokens_active_idx
    ON refresh_tokens(expires_at)
    WHERE revoked_at IS NULL;
