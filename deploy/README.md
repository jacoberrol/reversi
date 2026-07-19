# Deploying the netplay relay

The relay server (`netplay-server`) runs on an exe.dev VM behind the provider's
TLS proxy. Game clients connect to `wss://relay.netplay.oliverj.network`; the
admin API lives at `https://admin.netplay.oliverj.network`. **Both hostnames
resolve to the same IP and forward to the same server** on `127.0.0.1:8000`; the
server routes on the requested host (the proxy passes it as `X-Forwarded-Host`):
the admin host → REST admin API, everything else → the game WebSocket. Deploys
are **manual, via GitHub Actions** — you trigger them; nothing deploys on push.

```
game client  --wss://relay.netplay.oliverj.network--\
                                                     >  exe.dev proxy (TLS)
admin      --https://admin.netplay.oliverj.network--/        |
                                                             |  http/ws 127.0.0.1:8000
                                                             v
                                             netplay-server (systemd, VM)
                              routes by X-Forwarded-Host: admin.* → REST, else → game WS
```

**DNS + proxy:** add an `admin.netplay.oliverj.network` CNAME/record pointing at
the same VM and configure the exe.dev proxy to terminate TLS for it and forward
to the same `127.0.0.1:8000` (with `X-Forwarded-Host`). The server reads its
admin host from `NETPLAY_ADMIN_HOST` (the unit defaults it to
`admin.netplay.oliverj.network`).

## What the workflow does

`.github/workflows/deploy.yml` (job **Deploy relay**):

1. Builds a **static `x86_64-unknown-linux-musl`** `netplay-server` binary — no
   glibc coupling, so the VM needs no Rust toolchain.
2. Runs `deploy/ansible/playbook.yml` over SSH (as `exedev`, passwordless sudo):
   creates a locked-down `netplay` system user, installs the binary to
   `/usr/local/bin`, renders the token env file and the hardened systemd unit,
   enables + restarts the service, and health-checks `127.0.0.1:8000`.

The playbook is idempotent: it only restarts the service when the binary, the
tokens, or the unit actually change.

**State:** the unit declares `StateDirectory=netplay`, so systemd owns
`/var/lib/netplay` (writable despite `ProtectSystem=strict`); the server keeps
its SQLite database there (`NETPLAY_DB`) and migrates it on startup. The DB
survives redeploys and restarts; nothing in the playbook touches it.

## One-time setup

### 1. Create a dedicated CI deploy key

```sh
ssh-keygen -t ed25519 -f ~/.ssh/netplay-ci-deploy -N "" -C "netplay-ci-deploy"
```

Authorize the **public** key (`~/.ssh/netplay-ci-deploy.pub`) for the `exedev`
user on the VM (add it in the exe.dev key UI, or append to
`~/.ssh/authorized_keys`).

### 2. Set the secrets

| Secret | Value |
|---|---|
| `DEPLOY_SSH_KEY` | the **private** key: `gh secret set DEPLOY_SSH_KEY < ~/.ssh/netplay-ci-deploy` |
| `NETPLAY_ADMIN` | the admin account: `just set-admin <name>` (prompts for the password) |

The server seeds/rotates the admin account from `NETPLAY_ADMIN` on every boot
(idempotent argon2id upsert), so changing the secret + `just deploy` rotates the
admin password. The admin (e.g. the Go TUI) authenticates against the REST API
at `https://admin.netplay.oliverj.network`: `POST /admin/login` with
`{"name": "...", "password": "..."}` returns a bearer token (admin role only),
carried as `Authorization: Bearer <token>` on `GET /admin/{players,matches,stats}`.

### 3. Play

The relay is **accounts-only** with open registration — `just online` opens a
login screen where anyone can log in or create an account. No shared token, no
per-client setup. Regular accounts are `player` role; only the seeded admin can
touch the admin surface.

## Triggering a deploy

```sh
just deploy            # gh workflow run "Deploy relay"
# or: GitHub → Actions → "Deploy relay" → Run workflow
```

Watch it with `gh run watch` or in the Actions tab.

## Rolling back / rotating

- **Roll back:** re-run the workflow from an earlier commit (Actions → Run
  workflow → pick the ref).
- **Rotate the admin password:** `just set-admin <name>` (new password) + `just
  deploy` — the env file re-renders and the server re-upserts the admin on boot.
- **Revoke CI access:** remove the `netplay-ci-deploy` public key from the VM;
  it's independent of your personal keys.
