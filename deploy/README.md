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

### 2. Set the SSH key secret

| Secret | Value |
|---|---|
| `DEPLOY_SSH_KEY` | the **private** key: `gh secret set DEPLOY_SSH_KEY < ~/.ssh/netplay-ci-deploy` |

(The `NETPLAY_TOKENS` secret is set for you by `just rotate-token`, next.)

### 3. Mint the relay token (owner)

`just rotate-token` mints a fresh high-entropy token and stores it in **both**
places at once — your macOS login Keychain (for `just online`) and the
`NETPLAY_TOKENS` GitHub secret (for the server):

```sh
just rotate-token        # generates 2:<random>, → Keychain + NETPLAY_TOKENS secret
just deploy              # apply it on the running relay (see below)
```

The token format is `keyid:token` (`NETPLAY_TOKENS` accepts a comma-separated
list of them). Once the secret is set, the server accepts **only** those keys —
the built-in dev token stops working, so clients must present a matching token.
Rotate any time by re-running `just rotate-token` + `just deploy`.

### 4. Connect clients with the shared token

The client reads its credential from the `NETPLAY_TOKEN` env var (a single
`id:token`), falling back to the dev token when unset — the real secret is never
baked into the binary. As the owner, `just rotate-token` already put it in your
Keychain, so you just:

```sh
just online <name>       # reads the token from the Keychain automatically
```

Anyone you invite runs `just set-token` once (paste the token you share with
them out-of-band), then `just online <name>`.

`just online` prefers an already-exported `NETPLAY_TOKEN`, then the Keychain,
then the dev default — so `export NETPLAY_TOKEN=2:…` still works as a one-off
override (and is the portable option on non-macOS).

Share the token out-of-band with anyone you want to let in. This is a
deterrence gate (a distributed client can't keep a secret); real per-device
attestation is a later stage.

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
