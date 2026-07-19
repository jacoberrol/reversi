# Deploying the netplay relay

The relay server (`netplay-server`) runs on an exe.dev VM behind the provider's
TLS proxy. Clients connect to `wss://relay.netplay.oliverj.network`; the proxy
terminates TLS and forwards to the plain-`ws://` server bound on
`127.0.0.1:8000`. Deploys are **manual, via GitHub Actions** — you trigger them;
nothing deploys on push.

```
client  --wss://relay.netplay.oliverj.network-->  exe.dev proxy (TLS)
                                                        |  ws:// 127.0.0.1:8000
                                                        v
                                          netplay-server (systemd, VM)
```

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

## One-time setup

### 1. Create a dedicated CI deploy key

```sh
ssh-keygen -t ed25519 -f ~/.ssh/netplay-ci-deploy -N "" -C "netplay-ci-deploy"
```

Authorize the **public** key (`~/.ssh/netplay-ci-deploy.pub`) for the `exedev`
user on the VM (add it in the exe.dev key UI, or append to
`~/.ssh/authorized_keys`).

### 2. Add the repository secrets

| Secret | Value |
|---|---|
| `DEPLOY_SSH_KEY` | the **private** key: `gh secret set DEPLOY_SSH_KEY < ~/.ssh/netplay-ci-deploy` |
| `NETPLAY_TOKENS` | the real `keyid:token,keyid:token` string: `gh secret set NETPLAY_TOKENS` |

## Triggering a deploy

```sh
just deploy            # gh workflow run "Deploy relay"
# or: GitHub → Actions → "Deploy relay" → Run workflow
```

Watch it with `gh run watch` or in the Actions tab.

## Rolling back / rotating

- **Roll back:** re-run the workflow from an earlier commit (Actions → Run
  workflow → pick the ref).
- **Rotate tokens:** update the `NETPLAY_TOKENS` secret and re-run — the env file
  re-renders and the service restarts.
- **Revoke CI access:** remove the `netplay-ci-deploy` public key from the VM;
  it's independent of your personal keys.
