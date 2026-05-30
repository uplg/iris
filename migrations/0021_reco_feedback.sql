-- Per-user feedback on recommendations (Slice 5 of "For You").
--
-- Records explicit signals on catalogue candidates: `dismissed` (hide
-- from future shelves), and `followed` / `grabbed` (reserved for future
-- positive-signal weighting). The dismiss path is the one wired now.
--
-- Keyed on `catalog_id` (FK → catalog_items, CASCADE): a dismissal is
-- tied to the current catalogue row, so if a candidate is pruned
-- (`prune_stale`, 30 d) and later re-discovered with a fresh id, the
-- dismissal lapses — acceptable, since by then the trend that surfaced it
-- has usually moved on.

CREATE TABLE reco_feedback (
    user_id    BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    catalog_id BLOB      NOT NULL REFERENCES catalog_items(id) ON DELETE CASCADE,
    action     TEXT      NOT NULL CHECK (action IN ('dismissed', 'followed', 'grabbed')),
    created_at TIMESTAMP NOT NULL,
    PRIMARY KEY (user_id, catalog_id, action)
);

-- "what has this user actioned" lookups (dismiss exclusion).
CREATE INDEX reco_feedback_user_action_idx ON reco_feedback(user_id, action);
