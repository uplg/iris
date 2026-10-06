-- The account's last searches, shared by every client of the account (web,
-- TV). One row per distinct query (case-insensitive), newest first; the API
-- keeps the last few.
CREATE TABLE recent_searches (
    user_id      BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- lowercase, trimmed: the identity of the query
    query_key    TEXT      NOT NULL,
    -- as the user typed it last
    query        TEXT      NOT NULL,
    searched_at  TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, query_key)
);

CREATE INDEX recent_searches_user_recent ON recent_searches(user_id, searched_at DESC);
