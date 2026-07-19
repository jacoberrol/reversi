-- Accounts: stable named identities with a role.
--
-- Anonymous players (the shared token) have no row here. Named accounts do, and
-- `role` gates the admin/control surface (RBAC). `token_hash` is a hash of the
-- account's secret — the raw token is never stored.
CREATE TABLE users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    token_hash TEXT NOT NULL,
    role       TEXT NOT NULL DEFAULT 'player' CHECK (role IN ('player', 'admin')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
