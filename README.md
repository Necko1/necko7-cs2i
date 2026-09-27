# necko7 CS2 Integration

Windows companion for the existing necko7 backend and dashboard. Event/reward processing is intentionally **Coming soon**.

```text
Dashboard (existing Twitch session, channel OWNER)
  -> 5-minute code -> necko7-cs2i://pair?code=ABCD-2345
  -> Windows companion -> public key registration -> existing broadcaster
CS2 -> POST 127.0.0.1:31337/gsi -> local token validation
  -> exact-byte Ed25519 signature -> HTTPS necko7 /api/v1/cs2/gsi
  -> signature + timestamp + persisted replay checks -> future event boundary
```

## Prerequisites

Windows 10/11, Steam/CS2, WebView2, current Rust stable with MSVC build tools, Node.js 22.12+ and npm. PostgreSQL is required by the existing backend. Set up the existing Twitch application/bot and channel connection according to the backend README; this feature does not introduce another authentication flow.

## Run the three components

From the workspace root, in three separate PowerShell terminals:

```powershell
# Backend: configure necko7/.env using its .env.example first.
# DATABASE_URL must point at your development PostgreSQL.
# Twitch redirect URI: http://localhost:8080/api/v1/auth/callback
# APP_URL=http://localhost:8080; FRONTEND_URL=http://localhost:5173
cd .\necko7
cargo run
```

```powershell
cd .\necko7-frontend
npm ci
# Set BACKEND_URL=http://localhost:8080 and API_BASE_URL= in .env.local
npm run dev -- --host localhost --port 5173
```

```powershell
cd .\necko7-cs2i
npm ci
$env:CS2_API_URL='http://localhost:8080'
npm run tauri -- dev
```

Desktop defaults to the existing backend origin `https://7.necko.moe`. `CS2_API_URL` may override it with another HTTPS origin; HTTP is accepted **only in debug builds and only on loopback**. Redirects are disabled. `CS2_GSI_PORT` is a debug-only override; production uses 31337. Restart CS2 after first config installation. In development, enter the pairing code manually or pass a URI as a process argument; installed NSIS bundles register the scheme automatically.

Open the dashboard's **CS2 Integration** navigation item as channel OWNER. Editors/viewers cannot issue codes or manage devices. The page polls status every 15 seconds, counts down the code lifetime, and creates a replacement only while unpaired. A single active device is supported per channel. Unpair it before changing computers.

## Windows setup and lifecycle

The private Ed25519 seed is stored in Windows Credential Manager under service `moe.necko7.desktop`, account `ed25519`. It never enters JavaScript, JSON settings, logs, deep links, or backend storage. Public pairing metadata and the *local-only* GSI token live in Tauri's app data `settings.json`.

Steam roots come from HKCU/HKLM registry views. A VDF parser reads modern and legacy Steam libraries and app 730 manifests; only a single safe install-directory component is accepted. The app writes only `game/csgo/cfg/gamestate_integration_necko7.cfg`. Retry discovery supports CS2 installed or moved later. When moving, an old config is removed only if its complete contents still match ours. Other GSI files are untouched.

Normal launch shows the window. `--autostart` initializes services hidden in the tray. Start with Windows is disabled initially and requires an explicit Settings toggle. Closing hides the window when Minimize to tray is enabled. Tray Open focuses it; Exit cancels background work and terminates. Single-instance/deep-link plugins deliver subsequent links to the running process. Only the exact scheme/host, one `code` parameter, and a validated eight-character code are accepted.

## Security and failure behavior

- Five-minute random codes use an ambiguity-free alphabet, are stored as SHA-256 hashes, replaced per channel, and consumed inside a PostgreSQL transaction. Broadcaster locking also serializes generation, pairing and dashboard revocation.
- Devices use Ed25519 strict verification over **original HTTP body bytes**, not reconstructed JSON. Signed unpair messages have an explicit `action: unpair` and cannot be confused with GSI messages.
- Envelopes carry a random process session UUID, increasing sequence, and RFC3339 timestamp. The server allows +/-5 minutes and locks device rows during replay acceptance/revocation. Up to 32 recent sessions/device are retained for 11 minutes, longer than the timestamp window including future-dated requests. Expired records are pruned on ingestion; old sessions cannot become valid again after a process restart.
- Generation has a persisted 10-second cooldown. The existing governor library provides process-wide public-pairing (60/minute) and signed-ingestion (1000/second) limits. Production reverse proxies should also apply normal per-source limits; multi-replica deployments should account for each process having its own governor budget.
- Both GSI endpoints cap bodies at 256 KiB. The local listener binds only 127.0.0.1 and checks `auth.token` before signing. That local credential is removed from forwarded snapshots.
- The forwarding queue retains only the latest pending snapshot plus one in flight. Failures use an eight-second network timeout and five-second backoff; snapshots are dropped, never accumulated indefinitely. 401/403 clears the local association. No game-event extraction or rewards run here.
- Pairing responses are bounded and typed, UI metadata is rendered as text, avatar URLs require HTTPS, and the webview has a restrictive CSP with no general filesystem/network plugin permissions.

## Installer / distribution

```powershell
cd .\necko7-cs2i
npm ci
npm run tauri -- build --bundles nsis
```

Output: `src-tauri/target/release/bundle/nsis/necko7-cs2i_0.1.0_x64-setup.exe`.

The current-user NSIS bundle registers `necko7-cs2i://` and uses Tauri's normal uninstall registration cleanup. A minimal pre-uninstall hook first stops the running app through Tauri's standard process check, then invokes Rust cleanup to disable autostart and remove only an exact matching owned config. Upgrade uninstalls skip that cleanup. User settings/key are retained for reinstall; revoke the device in the dashboard if retiring the computer.

The GitHub Actions workflow `.github/workflows/release-installer.yml` builds the Windows x64 NSIS installer when a GitHub release is **published**, including prereleases, and attaches the generated `*-setup.exe` to that same release. Draft releases do not trigger it. It checks out the release tag, installs locked npm/Rust dependencies, and uses the automatic `GITHUB_TOKEN` with `contents: write`; no personal access token is required. Rerunning a failed workflow replaces an existing asset with the same name. The workflow must be committed and pushed before publishing a release that includes it. Before tagging a new version, update the versions in `package.json`/`package-lock.json`, `src-tauri/Cargo.toml`/`Cargo.lock`, and `src-tauri/tauri.conf.json`.

After publication, configure frontend `VITE_CS2_DOWNLOAD_URL` or Docker/runtime `CS2_DOWNLOAD_URL` with the installer's real HTTPS release-asset URL. Until configured, the dashboard clearly reports that the download is not published. Existing backend/frontend pipelines continue distributing Docker images. Installer code signing is not configured.

## Checks

```powershell
# Desktop
npm run typecheck
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
# Optional native test: creates/deletes only a unique test credential
cargo test --manifest-path src-tauri/Cargo.toml windows_credential_roundtrip -- --ignored

# Dashboard (from necko7-frontend)
npm run lint
npm run typecheck
npm run build
npm test
```

Backend tests include real Axum requests, PostgreSQL migration validation, code replacement/expiry/concurrent consumption, owner/editor authorization, raw-byte signatures, concurrent replay, session changes, timestamps and revocation. Use a **disposable** database:

```powershell
# From necko7, with PostgreSQL available on localhost:55437
$env:TEST_DATABASE_URL='postgres://postgres:cs2-test-only@127.0.0.1:55437/cs2_test'
$env:TWITCH_CLIENT_ID='test'
$env:TWITCH_CLIENT_SECRET='test'
$env:TWITCH_EVENTSUB_SECRET='test'
$env:APP_URL='http://localhost:8080'
$env:FRONTEND_URL='http://localhost:5173'
cargo check
cargo clippy --all-targets
cargo test -- --include-ignored
```

The existing SQLx migration workflow applies `20260927120000_cs2_integration.sql` on backend startup. Do not point integration tests at production.

## Troubleshooting / manual acceptance

- **Code expired / response lost:** get a fresh dashboard code. If the dashboard shows paired but the desktop lost the pairing response, revoke there and pair again.
- **Credential unavailable:** unlock Windows Credential Manager. If corrupt/missing, revoke the old device in the dashboard, then use Settings > Reset unavailable identity. No plaintext fallback is used.
- **CS2 missing/config missing:** install or move it via Steam, Retry discovery, and restart the game. Check cfg-folder write permissions.
- **Listener unavailable:** another process owns port 31337; close it and restart this app.
- **Cloud rejected GSI:** check the API origin and Windows clock. Revoked devices need a new code. Backend downtime is shown without terminating the listener.
- **Settings cannot be read:** preserve the file, revoke in the dashboard, then restore a valid backup or rename the corrupted settings file before restarting.
- **URI does not launch:** install the NSIS bundle. The dev server alone does not register a Windows protocol handler.

On a Windows test account, verify normal launch, opt-in autostart after sign-in, cold and warm pairing links, manual code entry, single-instance behavior, avatar/channel display, live CS2 snapshots, disconnect/reconnect, dashboard and desktop unpair, close-to-tray/Open/Exit, CS2 moved to another library, missing credentials, upgrade, and uninstall. Keep a second unrelated GSI file in the folder and confirm it survives uninstall. Test real Twitch OAuth and live CS2 separately from the isolated automated tests.

Implementation references: [Tauri deep linking/single instance](https://v2.tauri.app/plugin/deep-linking/), [Windows keyring abstraction](https://docs.rs/keyring/3.6.3/keyring/), [VDF parser](https://docs.rs/keyvalues-parser/0.2.4/keyvalues_parser/).

## Implementation map and dependencies

- Backend: `src/api/v1/cs2.rs` (routes, validation, signature/replay boundary), `src/api/v1/cs2_tests.rs` (database/API tests), `src/db/cs2.rs` (device query/model), SQL migration; existing router/OpenAPI and error enums extended. New direct dependencies: `ed25519-dalek`, `base64`, `rand`; test dependency `tower`.
- Dashboard: `src/pages/Cs2Page.tsx`, existing `src/lib/apiClient.ts`, `src/config.ts`, `src/App.tsx`, `src/components/layout/AppLayout.tsx`, runtime Docker config, and `tests/cs2.spec.ts`. No new dashboard dependencies.
- Desktop: Rust `app`, `cloud`, `deep_link`, `gsi`, `gsi_config`, `identity`, `settings`, `steam`, `tray`; TypeScript `main`/`ui`; Tauri capabilities/config, NSIS hook, VDF fixtures, and npm build setup.
- New desktop crates: Tauri deep-link/single-instance/autostart plugins; `tokio`, `tokio-util`, `axum`, `reqwest`, `ed25519-dalek`, `rand`, `base64`, `chrono`, `uuid`, `url`, `keyvalues-parser`, `subtle`, `tracing`, `tracing-subscriber`, `zeroize`, Windows `keyring`/`winreg`. npm: `@tauri-apps/api`, `@tauri-apps/cli`, `typescript`, `vite`. Lockfiles record exact resolutions.

Validation performed on Windows: all 90 backend tests including PostgreSQL integration tests passed; the final CS2-focused rerun also passed. All 60 existing/new dashboard Playwright tests passed. Desktop's seven normal tests passed, and the eighth Windows Credential Manager test passed when explicitly enabled. Cargo check, desktop fmt/strict Clippy, TypeScript checks and both production frontend builds passed. The NSIS installer was built. Backend Clippy and dashboard lint complete with existing warnings outside the CS2 code; repository-wide backend fmt check reports existing formatting differences, while the new CS2 modules pass rustfmt. Dashboard build retains its existing large-chunk warning. No installer installation or live Twitch/CS2 game session was performed.
