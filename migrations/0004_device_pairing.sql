-- Device pairing flow. A TV (or any headless device) hits POST
-- /auth/device/code to get a short user-facing CODE plus an opaque
-- DEVICE_ID. The user types CODE on the web UI; the TV polls with
-- DEVICE_ID and receives session tokens once the user has linked it.

CREATE TABLE device_codes (
    code            TEXT NOT NULL,        -- short, 8-char user-facing
    device_id       BLOB PRIMARY KEY NOT NULL, -- random uuid the device polls with
    created_at      TIMESTAMP NOT NULL,
    expires_at      TIMESTAMP NOT NULL,
    claimed_at      TIMESTAMP,
    claimed_by      BLOB REFERENCES users(id) ON DELETE CASCADE,
    label           TEXT,                 -- "Living room TV" set by user at link-time
    kind            TEXT NOT NULL DEFAULT 'unknown' -- "android-tv", "ios", "web", ...
);

CREATE UNIQUE INDEX device_codes_code_active_idx
    ON device_codes(code)
    WHERE claimed_at IS NULL;

-- Refresh tokens grow two optional columns so we can distinguish "phone web
-- session" from "TV in the living room" and let the user revoke devices
-- individually in the account UI.
ALTER TABLE refresh_tokens ADD COLUMN device_label TEXT;
ALTER TABLE refresh_tokens ADD COLUMN device_kind  TEXT;
