-- Per-user recommendation preferences (Slice 1 of the personalized
-- "For You" feature). Drives the first-login onboarding dialog and,
-- later, the reco scheduler's catalogue-slice union plus per-request
-- personalization.
--
-- The household is multi-user (~10 viewers across several families),
-- so preferences are strictly per-user: the shared surfaces stay the
-- library + scheduler, the personal surface is this table (see the
-- same split in `series_follows`).
--
-- `languages` is a JSON array using the iris_media::filename::Language
-- vocabulary ("french" / "english"), ordered most-preferred first.
-- `genres` is a JSON array of TMDB genre ids. `include_anime` is the
-- dedicated Anime toggle — anime affinity is tracked separately from
-- TMDB's "Animation" genre. `onboarding_completed` gates the
-- first-login dialog; onboarding is skippable, so a completed-but-
-- empty row is valid and expected.

CREATE TABLE user_preferences (
    user_id              BLOB      PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    languages            TEXT      NOT NULL DEFAULT '[]',
    genres               TEXT      NOT NULL DEFAULT '[]',
    include_anime        BOOLEAN   NOT NULL DEFAULT 0,
    onboarding_completed BOOLEAN   NOT NULL DEFAULT 0,
    created_at           TIMESTAMP NOT NULL,
    updated_at           TIMESTAMP NOT NULL
);
