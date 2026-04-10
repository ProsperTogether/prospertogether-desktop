# Prosper Together — Desktop Agent

Tauri 2 desktop capture app for Prosper Together. Records window / monitor / region captures
with drawing annotations and uploads them to the portal API.

## Tech

- **Frontend**: React 18 + TypeScript + Tailwind CSS + Vite
- **Backend**: Rust + Tauri 2
- **Capture**: Windows Graphics Capture (monitor-source with Rust-side crop) +
  `ffmpeg` sidecar for encoding
- **Audio**: `ffmpeg` dshow input
- **Transcription**: `whisper.cpp` sidecar

## Development

```powershell
npm install
scripts/download-sidecars.sh   # one-time: fetch ffmpeg + whisper binaries
npm run tauri dev
```

The dev build connects to `http://localhost:4000/api` (see `.env.development`).
Production builds use the DigitalOcean deployment (see `.env.production`).

## Release workflow

**Releases are automatic.** Every push to `master` that isn't a `[skip ci]` commit
triggers `.github/workflows/release.yml`, which:

1. Reads the latest `vX.Y.Z` git tag, bumps the patch, writes the new version into
   `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`,
   `package.json`, and `package-lock.json`.
2. Commits the bump with a `[skip ci]` message so it doesn't trigger another release.
3. Builds for `windows-x86_64`, `windows-aarch64`, and `macos-aarch64` (Apple Silicon)
   in parallel via a matrix job.
4. Tauri signs the updater assets (`.nsis.zip.sig` / `.app.tar.gz.sig`) using the
   private key from the `TAURI_SIGNING_PRIVATE_KEY` secret.
5. Assembles a `latest.json` manifest pointing at the uploaded assets with their
   signatures, and publishes a GitHub Release tagged `vX.Y.Z`.

Installed clients poll `https://github.com/ProsperTogether/prospertogether-desktop/releases/latest/download/latest.json`
on startup and every 30 minutes (guarded: no updates during active recording / upload /
transcription). When a new version is available the status bar shows an indicator;
clicking "Restart to update" runs the installer.

### First release after clone

The first push creates `v1.0.0` (or whatever `Cargo.toml` says) if there are no tags yet.
After that, each push increments the patch. To bump minor or major, manually edit the
version in `src-tauri/Cargo.toml` to something higher than the latest tag, then push —
the script will respect the higher value.

### Required GitHub Secrets

Configure these once in **Settings → Secrets and variables → Actions**:

- **`TAURI_SIGNING_PRIVATE_KEY`** — contents of `src-tauri/.tauri-private.key` (the whole
  multi-line base64 blob). This is the minisign private key that signs update manifests.
  **If you lose this key, no new builds can auto-update existing installs.** Back it up
  somewhere safe (e.g., 1Password).
- **`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`** — the password protecting the private key, or
  an empty string if the key is not password-protected.

The matching public key lives in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`
and is compiled into the app binary at build time.

### Generating a new signing key

If the private key is ever lost or compromised, generate a fresh keypair:

```powershell
npm run tauri signer generate -- -w src-tauri/.tauri-private.key
```

Then copy the public key output into `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`),
update the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret, and commit + push the `pubkey` change.
**Warning**: existing installed clients with the old pubkey will reject updates signed
with the new key, so you'll need to reinstall once manually on each client to bootstrap
the new key.

## Project layout

```
agent/
├── src/                  # React frontend (TypeScript)
│   ├── components/       # UI components
│   ├── hooks/            # React hooks (useSubmitRecording, useUpdateCheck, …)
│   ├── overlay/          # Drawing / border / region-picker overlay windows
│   ├── store/            # Zustand state
│   └── types/            # Shared TS types
├── src-tauri/            # Rust backend
│   ├── src/
│   │   ├── commands/     # #[tauri::command] handlers
│   │   ├── capture_target.rs
│   │   ├── state.rs
│   │   ├── migration.rs  # Bundle-identifier migration from UserFirst
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── .github/workflows/
│   └── release.yml       # CI: build + sign + publish GitHub Release
└── package.json
```

## License

Proprietary — Prosper Together.
