# LaunchPilot

Unified dashboard for installed game launchers (Steam, Epic, GOG, EA, Ubisoft, Battle.net, Riot) — see what needs updating, trigger updates from one place. Tauri v2 + Rust backend, vanilla TS frontend.

## Dev

```bash
npm install
npm run tauri dev
```

## Self-update signing (CI secrets)

Updates are signed with a minisign keypair generated via `npx tauri signer generate`. The public
key lives in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key must **never**
be committed — it's gitignored (`src-tauri/*.key`).

To wire up CI signing, add two repo secrets (Settings → Secrets and variables → Actions):

- `TAURI_SIGNING_PRIVATE_KEY` — contents of `src-tauri/launchpilot-updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password used when generating the key

Losing either means you can no longer publish updates users' existing installs will trust — back
both up somewhere durable (password manager) outside this repo.

## Releasing

Push a tag matching `v*` (e.g. `v0.1.1`) or run the `release` workflow manually. `tauri-action`
builds the signed Windows installer + `latest.json` and attaches them to a draft GitHub Release —
review and publish it. The app checks `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`
on startup.
