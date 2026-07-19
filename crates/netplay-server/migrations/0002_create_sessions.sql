-- Admin REST-API sessions: a bearer token maps to an account + role until it
-- expires. `token_hash` is a sha256 of the (high-entropy, random) bearer — the
-- raw token is never stored. Rows are pruned lazily on lookup.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    role       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);
