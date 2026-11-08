# Nebula Launcher

Cross-platform Minecraft launcher (Tauri + React) with **official Microsoft authentication only**.

It is inspired by Prism/Modrinth App UX and modern client polish. It does **not** implement cracked/offline play, account bypasses, or traffic proxies meant to dodge server rules.

## What this first cut includes

- Instance library, Mojang version manifest, library/asset install, JVM detect + Temurin download, Aikar/ZGC flags scaled to RAM
- Microsoft **device-code** login (Xbox Live → Minecraft services)
- Curated public Java server browser with live status, player counts, MOTD, icons, search, version/game-mode/region filters, pagination, and Quick Connect (`--server`)
- Modrinth and CurseForge search with Minecraft/loader-compatible versions, a 25-mod curated bulk installer, dependency-aware Modrinth install, and hash-verified JAR downloads
- Local JAR **heuristic** scan (string matches only; not antivirus), with launch blocked when a scan flags a mod
- Microsoft account switching/removal with token values redacted from frontend responses
- Themes, motion-blur toggle, Cmd/Ctrl+K command search, playtime + world folder vault
- Four-digit PIN locker with startup lock, manual lock, and current-PIN-protected change/removal
- Spotify PKCE sign-in, token refresh, official Web Playback SDK, track search and DJ prompt playback (Spotify Premium + Developer Client ID required)
- Crosshair PNG, keyword schematic JSON, CSS skin preview
- Optional Fabric companion sources in `overlay-mod/` (localhost FPS, not injection)

## What still needs your credentials / follow-up

| Feature | Status |
|---|---|
| Launching the game | Needs Java + MS token + version install. First asset pass can take a while. |
| Fabric, Quilt, Forge, and NeoForge loader install | Installed from their official APIs/installers; Minecraft version availability depends on each loader |
| Live FPS per mod | Needs the overlay mod running |
| Spotify in-process playback | Web Playback SDK implemented; requires Spotify Premium and a Developer Client ID |
| Touch ID / Windows Hello | PIN is implemented; OS biometrics not wired |
| CurseForge API | Search and file install are wired; requires a CurseForge developer API key |
| Mobile companion | Pairing string only |

The JAR scanner uses a short list of suspicious strings and can produce false positives or miss threats. It is not a malware database or a substitute for trusted sources. Modrinth installs resolve required dependencies with a 32-version limit; optional dependencies are not installed automatically.

Fabric and Quilt instances use their official metadata APIs to select a loader build. Forge and NeoForge use their official installer artifacts in Nebula's managed app-data directory. These compose loader profiles over the Mojang game version. Custom modpack import is not implemented yet.

The public server browser pings its curated server catalog directly. Status APIs such as MCSrvStat and Minetools report status for a known host rather than providing a searchable global directory; Minecraft-MP's directory API requires an API key. Server authentication mode is not reported reliably, and Quick Connect continues to use the launcher's official Microsoft-authenticated account.

For one-click Spotify sign-in in a published build, the app publisher must set the public Spotify Developer app `VITE_SPOTIFY_CLIENT_ID` in `.env` before building and register `http://127.0.0.1:17385/callback` as its Redirect URI. The Client ID is embedded in the frontend build; end users click **Connect Spotify** to authorize in their default browser, with tokens returned through the local PKCE callback. The Client ID is not a secret, but it must belong to the publisher's Spotify Developer app. The Web Playback SDK requires Spotify Premium.

For CurseForge search, obtain a developer API key from CurseForge and enter it in the Mods tab. Nebula does not persist the key.

## Run

Requirements: Node 20+, Rust stable, platform C toolchain (Xcode CLT on macOS).

```bash
export NEBULA_MS_CLIENT_ID=your-azure-public-client-id
npm install
source "$HOME/.cargo/env"
npm run tauri dev
```

Use `npm run app:dev` when developing; Vite hot-reloads frontend edits into the desktop window. The installed `.app` is a production build and updates only after `npm run app:build` and reinstalling the bundle.

On macOS, Nebula stores its accounts, settings, launcher metadata, runtimes, and game instances in the app-specific directory under `~/Library/Application Support`, created automatically when the app starts. The installed application is a regular Tauri `.app` bundle and opens directly into its WebView.

Register an Azure **public** client, Xbox Live sign-in, and Minecraft API. Accounts must **own Java Edition**.

## Signed application updates

Development builds use the existing Vite HMR setup through `npm run app:dev`. Production auto-updates are enabled only in release builds made with `npm run app:build:updater`; ordinary local builds do not check for updates or require signing credentials.

Before creating updater releases:

1. A local Tauri updater signing keypair is generated at `~/.tauri/nebula-launcher.key`; its public key is configured in `src-tauri/tauri.conf.json`. Keep the private key out of the repository; store it in release CI secrets when building releases elsewhere.
2. Publish Tauri's signed `latest.json` manifest and platform update archives at an HTTPS endpoint.
3. Replace the `https://updates.example.com/nebula/latest.json` placeholder in the ignored local `.env` or build environment with that HTTPS manifest URL. The placeholder in `tauri.conf.json` keeps the updater configuration explicit for normal builds; release builds reject it until a real URL is configured. `TAURI_SIGNING_PRIVATE_KEY` defaults to the local key path in `.env` and may be overridden in the environment. The private key environment variable may contain the key file path or key contents. Do not commit the private key or its contents.
4. Run `npm run app:build:updater`. The release build verifies signed-version metadata, generates signed updater artifacts, and includes the endpoint and public key. When an update is found, the app offers **Restart and Update**.

For GitHub Releases, the endpoint can be the repository's `releases/latest/download/latest.json` asset, provided the release workflow uploads the matching Tauri-generated archives, signatures, and manifest. This repository does not define a release repository or CI publishing workflow, so release hosting and secrets must be configured by the publisher. The endpoint remains empty until a real manifest is hosted; no dummy URL is configured because it would make update checks fail at runtime. Ordinary `npm run tauri build` does not require the endpoint or signing key. Never change the public key for an installed release unless publishing a migration build signed with the old key.

## Legal / product bounds

- Minecraft assets and the game jar come from Mojang; users must be licensed.
- OptiFine is not bundled (license). Link players to the official download.
- Spotify playback must go through Spotify’s SDK/app.
- No combat macros, queue snipers, or game-traffic geo-proxies.
