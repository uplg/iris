-- An invitation outlives the admin who made it: deleting that account used
-- to cascade away every invitation they ever created, used ones included
-- (which account came in through which invitation). `created_by` is now
-- cleared instead. SQLite can't alter a foreign key in place: rebuild.
CREATE TABLE invitations_new (
    id              BLOB PRIMARY KEY NOT NULL,
    token_hash      TEXT NOT NULL UNIQUE,
    created_by      BLOB REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMP NOT NULL,
    expires_at      TIMESTAMP NOT NULL,
    consumed_at     TIMESTAMP,
    consumed_by     BLOB REFERENCES users(id) ON DELETE SET NULL
);

INSERT INTO invitations_new (id, token_hash, created_by, created_at, expires_at, consumed_at, consumed_by)
SELECT id, token_hash, created_by, created_at, expires_at, consumed_at, consumed_by FROM invitations;

DROP TABLE invitations;
ALTER TABLE invitations_new RENAME TO invitations;

CREATE INDEX invitations_active_idx
    ON invitations(expires_at)
    WHERE consumed_at IS NULL;
