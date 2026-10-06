-- An admin's runtime on/off for a tracker built from providers.toml. No row =
-- on. A tracker disabled in the config is never built, whatever this says.
CREATE TABLE provider_overrides (
    provider_id  TEXT      PRIMARY KEY,
    enabled      INTEGER   NOT NULL,
    updated_at   TIMESTAMP NOT NULL,
    updated_by   BLOB      REFERENCES users(id) ON DELETE SET NULL
);
