# Xteink Unlocker

Desktop app that installs CrossPoint Reader on USB-locked Xteink X3/X4 devices by intercepting their OTA update mechanism. See [`xteink-unlocker-spec.md`](./xteink-unlocker-spec.md).

## Layout

```
crates/
  unlocker-core/    library: orchestrator, runtime, manifest server, DNS, certs, catalog, helper RPC client
  unlocker-helper/  privileged helper binary (LaunchDaemon, registered via SMAppService)
app/
  src/              React + Tailwind frontend
  src-tauri/        Tauri 2 shell, embeds the LaunchDaemon plist
scripts/
  bump-version.sh         bump tauri.conf + Cargo.toml + package.json (major|minor|patch)
  build-macos.sh          tauri build → inject helper → sign → notarize → update bundle
  upload-to-cloudflare.sh push artifacts to R2 + refresh latest.json
  release.sh              the whole pipeline: bump → build → commit → tag → push → upload
workers/
  releases/               Cloudflare Worker fronting the R2 bucket at
                          unlocker-releases.crosspointreader.com
```

## Bundle identifiers

- App: `com.sofriendly.crosspoint.unlocker`
- Helper: `com.sofriendly.crosspoint.unlocker.helper`

## Development

```bash
cd app && npm install

# headless checks
cargo check --workspace
npm run build

# dev mode (frontend only — helper integration needs the bundled flow below)
npm run tauri dev
```

In dev mode the helper isn't installed via SMAppService. To exercise the helper path locally:

```bash
cargo build --release -p unlocker-helper
sudo cp target/release/unlocker-helper /Library/PrivilegedHelperTools/
sudo cp app/src-tauri/LaunchDaemons/com.sofriendly.crosspoint.unlocker.helper.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/com.sofriendly.crosspoint.unlocker.helper.plist
```

## Signed builds

`scripts/build-macos.sh` runs the full pipeline:

1. `tauri build` — produces the `.app` and `.dmg`
2. Inject helper binary into `Contents/MacOS/unlocker-helper`
3. Inject LaunchDaemon plist into `Contents/Library/LaunchDaemons/`
4. Sign the helper, re-sign the bundle (so SMAppService sees a consistent signature)
5. Notarize `.app` and `.dmg` with `xcrun notarytool`, staple
6. Produce a signed `.tar.gz` for Tauri auto-update

Apple Team: **SoFriendly LLC (`2H66PPM438`)** — already wired into `tauri.conf.json` (`providerShortName`) and the build script's default identity.

### Setup

Copy `.env.local.example` to `.env.local` and fill in:

```bash
APPLE_ID=you@sofriendly.com
APPLE_PASSWORD=@keychain:AC_PASSWORD       # app-specific password, or @keychain:NAME
APPLE_TEAM_ID=2H66PPM438

# Optional, for auto-update bundle signing
# TAURI_SIGNING_PRIVATE_KEY=
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD=
```

`APPLE_SIGNING_IDENTITY` defaults to `Developer ID Application: SoFriendly LLC (2H66PPM438)`. Override via env if needed.

### One-shot build

```bash
npm run bundle
# or, with version bump:
./scripts/build-macos.sh patch
```

Output:
- Signed + notarized app at `target/release/bundle/macos/Xteink Unlocker.app`
- Signed + notarized DMG at `target/release/bundle/dmg/`
- Auto-update tarball at `target/release/bundle/XteinkUnlocker_<version>_darwin-aarch64.app.tar.gz` (signed if `TAURI_SIGNING_PRIVATE_KEY` is set)

### Cutting a release (full pipeline)

```bash
./scripts/release.sh patch
```

Bumps the version, builds + signs + notarizes, commits the version files, tags `vX.Y.Z`, pushes, and uploads to Cloudflare R2. After this completes, existing installs see the update on next launch (within 3s of opening the app) or when the user clicks **Check for updates** in the footer.

### Auto-update infrastructure

- **Endpoint:** `https://unlocker-releases.crosspointreader.com/latest.json`
- **Bucket:** `unlocker-releases` on Cloudflare R2 (account `73f82799694e2fad048f544e0be28c1c`)
- **Worker:** `workers/releases/` — deploy with `cd workers/releases && npx wrangler deploy`
- **Public key** (paste into the worker route or whatever consumes `latest.json` if you ever need to verify outside Tauri):
  ```
  dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEY1RDVFOTA2OTQzN0NCMTkKUldRWnl6ZVVCdW5WOVpyaHR0anNZRm5jNlBEaFk3WWJvbkpSdDdxbC9XSmQ5N0pZcitGK1d5YUMK
  ```

First-time setup:

1. Create the `unlocker-releases` R2 bucket in the SoFriendly Cloudflare account.
2. Create an R2 API token scoped to that bucket; paste keys into `.env.local`.
3. Add the DNS record for `unlocker-releases.crosspointreader.com` → Cloudflare Workers route.
4. `cd workers/releases && npm install && npx wrangler deploy`.
5. `./scripts/release.sh patch` to cut the first release.

## SMAppService flow at runtime

On first launch, the app calls `helper_status` (Tauri command). If the helper isn't yet registered, the UI shows an install screen; clicking the button calls `install_helper`, which invokes `SMAppService.daemon(plistName:).register()`. macOS prompts the user to approve in System Settings → Login Items & Extensions. Once approved, the helper LaunchDaemon starts, the app's status check sees the socket, and the wizard begins.

The relevant macOS bindings live in `app/src-tauri/src/smapp.rs` (objc2-based).

## Status

v0.1 — Real systems work in. Privileged helper drives `feth` virtual upstream + Internet Sharing + pfctl + dhcpd lease watching via shell-outs to system tools. DNS / HTTP / HTTPS spoofing servers run inside the helper, bound to the bridge IP. Orchestrator state machine drives the wizard end-to-end.

Untested against a real Xteink. Discovery item D11 (does macOS Internet Sharing accept `feth` as upstream on macOS 14/15?) is the remaining architectural risk.

## License

MIT.
