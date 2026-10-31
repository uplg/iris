-- A profile is a users row of its own, so everything personal (progress,
-- follows, preferences) stays keyed on users(id). `account_id` names the
-- account it hangs from; NULL is the account itself. Such a row has no way in
-- of its own: sign-in looks up accounts only. ADD COLUMN, never a users
-- rebuild: DROP TABLE would fire every ON DELETE CASCADE pointing here.
ALTER TABLE users ADD COLUMN account_id BLOB REFERENCES users(id) ON DELETE CASCADE;

CREATE INDEX users_account_idx ON users(account_id) WHERE account_id IS NOT NULL;
