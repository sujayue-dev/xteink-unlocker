# Xteink Unlocker

Desktop app that installs CrossPoint Reader on USB-locked Xteink X3/X4 devices by intercepting their OTA update mechanism.

- [`xteink-unlocker-spec.md`](./xteink-unlocker-spec.md) — product spec
- [`INTEGRATION.md`](./INTEGRATION.md) — guide for pointing Unlocker at a different firmware (catalog + image requirements, including the X3 eFuse blk validity workaround)
- [`crosspointreader-com-catalog-spec.md`](./crosspointreader-com-catalog-spec.md) — catalog endpoint schema and rationale

## How it works

1. The Mac becomes a Wi-Fi hotspot via a `feth` virtual upstream + Internet Sharing.
2. The privileged helper runs DNS / HTTP / HTTPS listeners bound to the bridge IP. DNS spoofs the locale's Xteink API host (`api-prod.xteink.cc` / `.cn`); HTTPS uses a self-signed cert with the right SAN (stock doesn't validate the chain).
3. The user taps **Check for Updates** on the device. The spoofed `/api/v1/check-update` returns a manifest pointing at firmware Unlocker also serves over plain HTTP on the bridge IP.
4. The device installs via its own `esp_https_ota` flow.

The firmware Unlocker serves comes from a **catalog** — currently `https://crosspointreader.com/api/catalog`. For other firmwares, see [`INTEGRATION.md`](./INTEGRATION.md).

## Layout

```
crates/
  unlocker-core/    library: orchestrator, runtime, manifest server, DNS, certs, catalog, helper RPC client
  unlocker-helper/  privileged helper binary (runs as root via osascript admin prompt)
app/
  src/              React + Tailwind frontend
  src-tauri/        Tauri 2 shell
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

In dev mode the bundled helper isn't available. To exercise the helper path locally, build it and let the app launch it on demand via the admin prompt:

```bash
cargo build --release -p unlocker-helper
```

The signed app bundles the helper binary at `Contents/MacOS/unlocker-helper` and launches it as root on demand via `osascript`'s admin password prompt — no LaunchDaemon, no SMAppService, no provisioning profile. The helper writes a crash-recovery state file to `/var/db/com.sofriendly.crosspoint.unlocker.helper.state.json` and reverses any leftover changes (pfctl rules, `feth` interfaces, NAT plist) on next launch.

## Signed builds

`scripts/build-macos.sh` runs the full pipeline:

1. `tauri build` — produces the `.app` and `.dmg`
2. Inject the helper binary into `Contents/MacOS/unlocker-helper`
3. Sign the helper, re-sign the bundle for a consistent signature
4. Notarize `.app` and `.dmg` with `xcrun notarytool`, staple
5. Produce a signed `.tar.gz` for Tauri auto-update

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

## Helper launch at runtime

When the orchestrator needs the helper, the app shells out to `osascript` with an admin password prompt and exec's `unlocker-helper` from inside the bundle as root. This replaced an earlier SMAppService/LaunchDaemon design that ran into provisioning-profile requirements on macOS 26. The helper exits when the app does (or via explicit teardown RPC); next session, a fresh prompt.

## Status

Real systems work in. Privileged helper drives `feth` virtual upstream + Internet Sharing + pfctl + dhcpd lease watching via shell-outs to system tools. DNS / HTTP / HTTPS spoofing servers run inside the helper, bound to the bridge IP. Orchestrator state machine drives the wizard end-to-end. Working installs against stock X3 require the bootloader-validation override described in [`INTEGRATION.md` §2.4](./INTEGRATION.md#24-the-x3-efuse-blk-validity-gotcha-critical) — already shipped in CrossPoint.

## License

MIT.
