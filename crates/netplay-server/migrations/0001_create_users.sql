-- Accounts: stable named identities with a role.
--
-- Anonymous players (the shared token) have no row here. Named accounts do, and
-- `role` gates the admin/control surface (RBAC). `password_hash` is an argon2id
-- PHC string (salt embedded) — the raw password is never stored.
CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'player' CHECK (role IN ('player', 'admin')),
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
