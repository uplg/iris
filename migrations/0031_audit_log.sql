-- Persistent record of sensitive actions (deletions, password resets,
-- display-name changes, admin-triggered GC) — until now these only hit
-- ephemeral `tracing::` logs, which rotate out and aren't queryable from
-- the admin UI. `actor_id` is whichever user performed the action (most
-- household members can delete their own torrents; only admins can read
-- this log back via `/admin/audit-log`). No `ON DELETE CASCADE`: an audit
-- trail must outlive the account that produced it.
CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_id        BLOB    NOT NULL REFERENCES users(id),
    action          TEXT    NOT NULL,
    resource_type   TEXT    NOT NULL,
    resource_id     TEXT,
    details         TEXT,
    created_at      TIMESTAMP NOT NULL
);

CREATE INDEX audit_log_created_at_idx ON audit_log(created_at DESC);
