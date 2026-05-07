# Xteink Unlocker

Desktop app that installs CrossPoint Reader (and other compatible firmwares) on USB-locked Xteink X3/X4 devices by intercepting their OTA update mechanism. Works against stock Xteink firmware as well as already-flashed CrossPoint and CrossInk devices, enabling cross-flashing between firmwares.

- [`xteink-unlocker-spec.md`](./xteink-unlocker-spec.md) — product spec
- [`INTEGRATION.md`](./INTEGRATION.md) — guide for pointing Unlocker at a different firmware (catalog + image requirements, including the X3 eFuse blk validity workaround)
- [`crosspointreader-com-catalog-spec.md`](./crosspointreader-com-catalog-spec.md) — catalog endpoint schema and rationale

## How it works

1. The Mac becomes a Wi-Fi hotspot via a `feth` virtual upstream + Internet Sharing. (Windows uses Mobile Hotspot — see below.)
2. The privileged helper runs DNS / HTTP / HTTPS listeners bound to the bridge IP. DNS spoofs three hostnames: the locale's Xteink API host (`api-prod.xteink.cc` / `.cn`), `api.github.com`, and `unlocker.crosspointreader.com`. HTTPS uses a real Let's Encrypt cert for `unlocker.crosspointreader.com` — trusted by ESP-IDF's `esp_crt_bundle`, so both stock and CrossPoint/CrossInk firmwares accept it.
3. The user taps **Check for Updates** on the device. Depending on what's running:
   - **Stock Xteink** → hits `https://api-prod.xteink.{cc,cn}/api/v1/check-update`. We return a manifest pointing at a plain-HTTP firmware URL on the bridge IP.
   - **CrossPoint / CrossInk** → hits `https://api.github.com/repos/{owner}/{repo}/releases/latest`. We return a GitHub-shaped releases JSON with all known asset variants (`firmware.bin`, `firmware-{tiny,xlarge,no_emoji}-…bin`) all pointing at the same HTTPS firmware URL on `unlocker.crosspointreader.com`. The device's variant matcher picks one and downloads.
4. Whichever name the device picked, the bytes returned are whatever firmware the user chose in the unlocker UI — the asset name is decoupled from the bytes, which is what enables cross-flashing.
5. The device installs via its own `esp_https_ota` flow.

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
  build-macos-dev.sh      same as above but skips notarization (faster local iteration)
  build-windows.ps1       Windows equivalent of build-macos.sh (NSIS + MSI + signtool)
  upload-to-cloudflare.sh push artifacts to R2 + refresh latest.json
  upload-to-cloudflare.ps1 Windows equivalent
  release.sh              the whole pipeline: bump → build → commit → tag → push → upload
firmware-patches/         pre-patched firmware bins for cases the catalog can't cover
                          (e.g. the X3 eFuse blk validity workaround)
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

On Windows the equivalent is a UAC prompt: the app calls `Start-Process -Verb RunAs` to launch `unlocker-helper.exe`, which carries a `requireAdministrator` manifest. The RPC channel is a named pipe at `\\.\pipe\com.sofriendly.crosspoint.unlocker.helper` instead of a Unix socket.

## Windows

Windows uses Mobile Hotspot (`NetworkOperatorTetheringManager`) for AP + NAT + DHCP in one step — no equivalent of macOS's "enable Internet Sharing in System Settings" handoff. The host always lands at `192.168.137.1` and clients are on `192.168.137.0/24`. Device discovery scans the system ARP table under that subnet rather than reading a `dhcpd_leases` file.

Requirements:
- Windows 10 1607 or newer (Windows 11 recommended).
- A Wi-Fi adapter that supports Mobile Hotspot.
- An active internet connection — Windows' tethering API requires a profile to share. (macOS bypasses this with a fake `lo0` upstream; Windows doesn't allow it.)

### Build

```powershell
# bumps version (optional), builds helper + app, signs both installers
.\scripts\build-windows.ps1 patch
.\scripts\upload-to-cloudflare.ps1
```

The PowerShell scripts mirror the macOS pipeline: bump version, `cargo build --release -p unlocker-helper`, `npm run tauri -- build` (NSIS + MSI, picks up `app/src-tauri/tauri.windows.conf.json`), `signtool` for both installers using the Sectigo USB token, then push to R2 and merge a `windows-x86_64` entry into `latest.json` while preserving `darwin-aarch64`.

## Debugging

The helper writes a verbose log of every DNS query, every HTTP/HTTPS request, and every state transition. This is the primary tool for diagnosing OTA failures and noticing when firmware OEMs change their API shape.

- **macOS:** `/tmp/unlocker-helper.log`
- **Windows:** `C:\ProgramData\CrossPoint\unlocker-helper\unlocker-helper.log`

The file is overwritten on each helper launch. Bump verbosity by setting `RUST_LOG=unlocker_core=debug,unlocker_helper=debug` in the environment that launches the app.

What gets logged on every session:

- `dns query host=… spoofed=true|false` — every DNS lookup the device makes. New unfamiliar hosts here mean the firmware is talking to an endpoint we don't yet spoof.
- `http request method=… uri=… host=… ua=…` — middleware logs every HTTP/HTTPS hit before any handler runs, including ones that fall through to `catch_all`.
- `stock device requested update` / `device requested update via GitHub API` / `device activate` — handler-level logs for the recognized OTA endpoints.
- `unknown request — returning ok stub` (warn level) — fallback handler. Returns a `{code:0,message:"ok",data:{}}` envelope on any unrecognized path so the device doesn't see a 404. Watch this in logs to find new endpoints to promote to real handlers.
- `firmware download requested` / `serving firmware` — the actual OTA payload transfer. Includes the device's `x-esp32-*` headers, range, and SHA verification of the bytes on disk against the catalog hash.

For OTA install failures, the helper log shows everything *we* see; it can't show the device-side `esp_err_t` from `esp_https_ota_*`. For that, attach USB serial to the device (`screen /dev/cu.usbmodem* 115200` on macOS) and watch the firmware's own `LOG_ERR("OTA", …)` lines.

## Status

Real systems work in. Privileged helper drives `feth` virtual upstream + Internet Sharing + pfctl + dhcpd lease watching via shell-outs to system tools. DNS / HTTP / HTTPS spoofing servers run inside the helper, bound to the bridge IP. Orchestrator state machine drives the wizard end-to-end. Stock Xteink → CrossPoint and CrossPoint → CrossInk both flash cleanly. CrossInk → CrossPoint is currently failing at `esp_https_ota_finish` for reasons not yet determined from CrossInk's source — see [`crossink-cross-flash-analysis.md`](./crossink-cross-flash-analysis.md). Working installs against stock X3 require the bootloader-validation override described in [`INTEGRATION.md` §2.4](./INTEGRATION.md#24-the-x3-efuse-blk-validity-gotcha-critical) — already shipped in CrossPoint.

## License

MIT.
