-- Migration 0031 declared "an audit trail must outlive the account that
-- produced it" but kept a plain FK on actor_id — which, with
-- foreign_keys=ON, BLOCKS deleting any user that ever produced an audit
-- row. Rebuild the table without the constraint (SQLite can't drop a
-- single FK in place); actor_id keeps the raw uuid so the trail stays
-- attributable, and reads fall back to a placeholder display name once
-- the account is gone.
CREATE TABLE audit_log_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_id        BLOB    NOT NULL,
    action          TEXT    NOT NULL,
    resource_type   TEXT    NOT NULL,
    resource_id     TEXT,
    details         TEXT,
    created_at      TIMESTAMP NOT NULL
);

INSERT INTO audit_log_new (id, actor_id, action, resource_type, resource_id, details, created_at)
SELECT id, actor_id, action, resource_type, resource_id, details, created_at FROM audit_log;

DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;

CREATE INDEX audit_log_created_at_idx ON audit_log(created_at DESC);
