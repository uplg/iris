-- Friendships hang on profiles (users rows). A declined request keeps its row:
-- the asker keeps seeing it pending, is told nothing, and cannot ask again.
CREATE TABLE friendships (
    requester_id    BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    addressee_id    BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TIMESTAMP NOT NULL,
    accepted_at     TIMESTAMP,
    declined_at     TIMESTAMP,
    PRIMARY KEY (requester_id, addressee_id),
    CHECK (requester_id <> addressee_id)
);

CREATE INDEX friendships_addressee_idx ON friendships(addressee_id);
CREATE UNIQUE INDEX friendships_pair_idx
    ON friendships(min(requester_id, addressee_id), max(requester_id, addressee_id));

-- What a profile watches reaches its friends only once it says so. What it
-- adds to the library was already visible to everyone ("added by").
ALTER TABLE users ADD COLUMN share_watching BOOLEAN NOT NULL DEFAULT 0;

-- Titles kept from friends: by collection, by release when it has none.
CREATE TABLE friends_hidden (
    user_id         BLOB      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    collection_id   BLOB      REFERENCES collections(id) ON DELETE CASCADE,
    infohash        TEXT,
    created_at      TIMESTAMP NOT NULL,
    CHECK ((collection_id IS NULL) <> (infohash IS NULL))
);

CREATE UNIQUE INDEX friends_hidden_collection_idx
    ON friends_hidden(user_id, collection_id) WHERE collection_id IS NOT NULL;
CREATE UNIQUE INDEX friends_hidden_release_idx
    ON friends_hidden(user_id, infohash) WHERE infohash IS NOT NULL;
