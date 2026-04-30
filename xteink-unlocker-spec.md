# Xteink Unlocker

**A no-cable install path for CrossPoint Reader.**

Version 0.1 — Draft Product Spec

> **Context:** Recent Xteink shipments have disabled USB flashing, leaving the WebSerial flasher unusable on newly-purchased devices. Xteink Unlocker restores the install path by intercepting the device's own OTA update mechanism and serving CrossPoint as the response. For users with USB-locked devices, this is the only remaining path to CrossPoint.

---

## 1. Overview

Xteink Unlocker is a desktop application that installs CrossPoint Reader onto a stock Xteink X3 or X4 e-reader by intercepting the device's own OTA update request and substituting the CrossPoint firmware in place of Xteink's stock release.

### Why this exists

Recent Xteink shipments have disabled USB flashing — either through firmware changes that block the bootloader's USB DFU mode, or through hardware changes that physically prevent the boot-strap pins from being pulled into download mode. Users who buy a new Xteink today cannot use the existing WebSerial flasher at crosspointreader.com to install CrossPoint. The only path to CrossPoint that remains is the device's own built-in OTA mechanism — and that mechanism, by default, only accepts firmware from Xteink's update servers.

Unlocker closes that gap. It impersonates Xteink's update server on the user's local network, lets the device "check for updates" as it normally would, and serves CrossPoint as the response. The device installs CrossPoint using its own native, vendor-supplied OTA flow — no USB connection, no boot-strap pins, no WebSerial.

### Relationship to the WebSerial flasher

Unlocker does not replace the WebSerial flasher. Both tools have their place:

- **WebSerial flasher** is the right path for users with older Xteink shipments where USB flashing still works, for users who want a full-flash backup before installing CrossPoint, and for recovery from a bricked device.
- **Unlocker** is the only available path for users with recent shipments where USB flashing has been disabled. It is also a viable path for users with older devices who can't or won't use WebSerial (Safari/Firefox-only, charge-only USB cables, managed corporate machines).

The crosspointreader.com flash page should detect or ask which scenario the user is in and route them appropriately. If the device is from a recent shipment where USB is locked, Unlocker is not the alternative — it's the answer.

### How it works

Unlocker turns the user's Mac into a Wi-Fi hotspot, runs a fake DNS resolver and HTTPS server that impersonates Xteink's update API (`api-prod.xteink.cc` and `api-prod.xteink.cn`), and serves the appropriate CrossPoint OTA payload when the user taps "Check for Updates" on their device. Stock firmware fetches and installs the substituted firmware using its own built-in OTA flow — the same flow it would use to install a legitimate Xteink update.

---

## 2. Goals and non-goals

### Goals

- **Restore the install path that Xteink has closed off.** For users with USB-locked devices, Unlocker is the only way to get CrossPoint installed. The product's success is measured by these users successfully completing the install, not by an abstract "ease of use" metric.
- **Self-contained installs.** No phone tether, no Ethernet adapter, no hardware shopping. Unlocker creates a virtual upstream interface internally, so the only thing the user needs is a Mac and the device they're flashing. Internet is needed during onboarding to download firmware; the install itself runs on a purely local network created by the app.
- Single guided path from app launch to a CrossPoint-running device in under 15 minutes for a first-time user.
- No router changes, no command-line work, no manual `dnsmasq` or `pfctl` configuration.
- Clear, recoverable failure modes — every error state has a defined exit and the user always knows what state their machine and device are in.
- **Resilient to Xteink's countermeasures.** Xteink has already made one move to lock down their devices (disabling USB). They may make others — for example, signing OTA payloads, or rotating API hostnames. Unlocker's design should anticipate this: the artifact catalog and intercept hostnames are config rather than code, so a counter-update can ship as a config bump rather than a full release. See §14 for the adversarial dynamics this creates.
- Auditable: every intercepted request, served response, and bytes transferred is logged and exportable as a session bundle.
- Tight integration with the CrossPoint release pipeline so new firmware releases require only a config bump in Unlocker, not a code change.

### Non-goals

- **Bypassing signed firmware.** Unlocker is built around the fact that stock Xteink does not sign its OTA payload. If Xteink ships a firmware update that introduces signing, Unlocker will detect this and refuse to operate.
- **Windows and Linux support in v1.** Architecturally feasible but deferred. macOS ships first because Internet Sharing is the cleanest hotspot story.
- **Bring-your-own-firmware.** v1 only serves blobs from the CrossPoint release catalog. Arbitrary user-supplied `.bin` files are out of scope.
- **Recovery from bricked devices is best-effort, not guaranteed.** For users with working USB, Unlocker directs to the WebSerial flasher's full-flash backup/restore. For users with USB-locked devices — Unlocker's primary persona — WebSerial recovery is not available. This is a real and significant risk, and Unlocker must be honest about it: a failed install on a USB-locked device may result in a brick that can only be recovered via UART (if pads are exposed) or by the device's own OTA rollback partition (if it survived). The consent screen in §4.1 must communicate this explicitly.

---

## 3. Target users and personas

**Primary: Locked-Out Buyer.** Just bought a new Xteink X3 or X4. Discovered CrossPoint, tried to install it via the WebSerial flasher, and found that USB flashing doesn't work on their device — the bootloader rejects the connection, or the flasher fails partway through, or the device doesn't enter download mode at all. Has read enough forum threads and GitHub issues to know this is a hardware/firmware lockdown on recent shipments, not user error. Frustrated, possibly considering returning the device. Unlocker is their last viable path to the firmware they bought the device for.

This persona's emotional state matters for the UX: they are arriving at Unlocker after a failed install attempt, often with the device in an unknown state (sometimes partially flashed, sometimes not). Unlocker should explicitly acknowledge this — "Did the WebSerial flasher fail for you? You're in the right place." — and avoid treating Unlocker as an exotic alternative when for many users it's the only option.

**Secondary: Cable-Averse Reader.** Has an older Xteink where USB flashing still works in principle, but doesn't have a USB-C data cable, is on Safari, or is intimidated by reset/power-button choreography. WebSerial would work for them with effort; Unlocker is easier.

**Tertiary: Existing CrossPoint User.** Already running CrossPoint on one device, buying a second. Knows the project, trusts the firmware. Wants to skip the cable hunt for a five-minute install. May or may not realise their new device has USB locked down until they try.

**Quaternary: Reviewer / Recommender.** Tech YouTuber, e-reader blogger, or forum power user. Will install on multiple devices in sequence. Cares about reliability and speed of teardown/setup between installs.

Notable non-target: **the developer.** Anyone capable of running `idf.py flash` on a device with exposed UART pads doesn't need Unlocker. Unlocker optimises for the user who has never opened a terminal.

---

## 4. End-to-end user flow

Unlocker presents a linear wizard. Each step has an explicit state in the orchestrator state machine; users can move forward only when the state's preconditions are met. Backward navigation and Cancel-with-Cleanup are always available.

### 4.1 Welcome and consent

Plain-language explanation of what Unlocker will do and what's at stake:

- Modify network settings on the Mac, including enabling Internet Sharing to create a Wi-Fi hotspot. **Your Mac's Wi-Fi connection will be temporarily disconnected during the install.** Wired Ethernet, if present, is unaffected. The Wi-Fi connection is restored automatically when the install completes or is cancelled.
- Replace the firmware on the user's device with CrossPoint
- **Risk disclosure specific to USB-locked devices.** If the user's device has the USB lockdown that motivated Unlocker's existence, recovery from a failed install is significantly harder than for older devices. The consent screen explains this honestly: in the worst case, a failed install can result in a permanently non-functional device. The user must check a separate "I understand the recovery limitations" box, in addition to the general consent box.
- A "use the WebSerial flasher instead" link, with copy that helps the user decide whether WebSerial is even an option for them ("If you've already tried WebSerial and it failed because your device wouldn't enter download mode, that's the lockdown Unlocker is designed for — continue here.")

### 4.2 Device and region selection

The user picks two things on this screen: device model and region (English/Chinese stock firmware). This step requires internet access on the Mac and runs before any network changes happen.

**Why these selections matter to Unlocker:**

- **Device model** (X3 / X4) determines which `device_type` value Unlocker expects in the OTA request and which model code (`X3` / `X4`) Unlocker uses when constructing the spoofed manifest's filename pattern, in case stock validates URL structure (D6).
- **Region** (English overseas / Chinese domestic) determines which Xteink API hostname Unlocker spoofs (`api-prod.xteink.cc` for English, `api-prod.xteink.cn` for Chinese) and which locale code (`EN` / `CH`) appears in the spoofed filename. Unlocker only spoofs the one hostname matching the selected region — getting this wrong means the device's request goes to real Xteink (or nowhere) and Unlocker simply never receives anything to respond to.

The user picks from two cards (Xteink X3 / Xteink X4). Helper text: "Not sure? Look at the back of your device — the model number is printed there."

Region selection: "Which language did your device come with from the factory?" with two options (English and Chinese).

### 4.3 Channel selection

The user picks one of three CrossPoint channels — **Stable**, **Beta**, or **Insider (nightly)** — by tapping a single card. Unlocker auto-selects the latest release on that channel; there is no version-by-version picker.

Unlocker fetches the catalog from `https://crosspointreader.com/api/catalog` (TLS-pinned) at this point. If the catalog fetch fails, Unlocker shows a clear error and asks the user to check their internet connection — there's no point proceeding to hotspot setup if no firmware is available to serve.

On selection, Unlocker downloads the chosen firmware blob, verifies its SHA-256 against the catalog, and caches it on disk at `~/Library/Application Support/XteinkUnlocker/firmware/{sha256}.bin`. Cached blobs are reused on subsequent runs.

After this step completes, every subsequent step is fully offline. The Mac can lose its internet connection (and will, when the hotspot starts) without affecting the install.

### 4.4 Connect — hotspot + tap-Check

After firmware download, Unlocker brings up the local network and shows the user a single screen with everything they need to do. The screen has four distinct phases driven by the orchestrator state:

**Phase A — preparing/hotspot starting.** Unlocker creates a `feth` virtual interface and writes the Internet Sharing NAT plist. The screen shows "Setting up the local network…" with a progress bar.

**Phase B — enable Internet Sharing.** Unlocker cannot programmatically start Internet Sharing on modern macOS (launchctl kickstart is no longer reliable). Instead, it shows the user step-by-step instructions to enable it manually:

1. Open **System Settings → General → Sharing → Internet Sharing**
2. Set **Share your connection from** to `feth7` and check **Wi-Fi** in the "To devices using" list
3. Toggle Internet Sharing on

Unlocker polls for `bridge100` to appear and auto-advances once it does (up to 5 minutes).

**Phase C — connect device.** Once the bridge is up, the screen shows:

- **SSID** and **password** (top, in two info boxes)
- **Step 1:** Join the network on your Xteink — checks off when a DHCP lease for an Espressif MAC appears
- **Step 2:** Tap Settings → System → Check for Updates on the Xteink — Unlocker auto-advances the moment the manifest request hits its HTTPS server

There is no separate "Confirm and install" step. The act of tapping Check for Updates on the device *is* the user's confirmation.

**Phase D — waiting for check.** After device joins, the helper installs a `pfctl` anchor redirecting bridge100 UDP/TCP port 53 to its internal DNS listener on port 5353, and spawns DNS, HTTP, and HTTPS servers bound to the bridge IP.

**Hotspot mechanics:**

1. The privileged helper creates a `feth` (fake ethernet) interface configured with `10.99.99.1/24`, satisfying Internet Sharing's requirement for an upstream interface without needing a phone tether or Ethernet adapter.
2. The helper writes `/Library/Preferences/SystemConfiguration/com.apple.nat.plist` with the hotspot SSID and password configured for the `feth` upstream.
3. The user manually enables Internet Sharing in System Settings. `bridge100` comes up; the Wi-Fi card reconfigures into AP mode.
4. The helper installs a `pfctl` anchor redirecting bridge100 UDP/TCP port 53 to its internal DNS listener on port 5353.
5. The helper spawns DNS, HTTP, and HTTPS servers bound to the bridge IP. HTTPS uses a self-signed certificate — stock Xteink firmware does not validate TLS certificates during OTA checks.

**DHCP watching.** The helper polls `/var/db/dhcpd_leases` for new leases on the bridge interface. When a lease appears with a MAC matching the Espressif OUI range, Unlocker checks off Step 1 and surfaces the device IP next to it.

**Manifest detection.** When the device's `GET /api/v1/check-update` request hits the helper's HTTPS server, the helper signals the unprivileged main via the `WaitManifest` RPC, which advances the orchestrator out of `AwaitingDeviceRequest` and into `Serving`.

**Misconfiguration safety.** If the user picked the wrong model or region in §4.2, the device's request will go to a hostname Unlocker isn't spoofing, and nothing reaches Unlocker at all. The Connect screen will sit on Step 2 indefinitely. After a 5-minute timeout, Unlocker surfaces a diagnostic with the most likely cause ("If your device is running Chinese firmware, go back and select Chinese in the previous step.").

### 4.5 Install progress

After the manifest request is detected, Unlocker shows a progress panel with four stages:

- Armed — servers up, awaiting binary fetch
- Manifest served — device received our manifest; will fetch binary next
- Streaming firmware — binary download in progress
- Device flashing — binary fully streamed; device is writing the OTA partition and rebooting

A live event log on the same screen shows per-request entries (timestamp, request line, response code, bytes). A "Cancel and clean up" button is always present.

The orchestrator advances between stages on real signals from the helper (`WaitManifest`, `WaitFirmware`) — not timers.

### 4.6 Verification

After the device reboots, Unlocker guides the user to:

- Check that the new firmware is running (visual confirmation: CrossPoint's UI is recognisably different from stock — Lyra theme, version string in Settings → System)
- Run a brief sanity check (open a book, change a font size)

If verification fails, Unlocker captures a full diagnostic bundle and surfaces recovery options based on the user's situation:

- **If their device's USB still works:** direct to WebSerial full-flash restore.
- **If their device is USB-locked:** explain that the device may self-recover on next boot via the OTA rollback partition (ESP-IDF's standard behaviour when a new partition fails to mark itself as "valid"), and provide instructions for forcing a rollback. If that fails, document the UART recovery path for users willing to open the device, and connect them with the CrossPoint community for assistance.

Honest framing matters here: Unlocker should not promise recovery it cannot deliver.

### 4.7 Teardown

Unlocker:

- Stops its DNS and HTTPS servers
- Removes the `pfctl` redirect rules
- Walks the user through disabling Internet Sharing on the Mac
- Offers to delete cached firmware blobs and session logs (default: keep, in case the user wants to share for debugging)

Final screen: "CrossPoint is installed. Welcome." With links to the CrossPoint docs, Calibre plugin, and font builder.

---

## 5. Xteink OTA protocol

Reverse-engineered from publicly observable API behaviour.

### 5.1 Endpoints

Two production hosts, representing entirely separate firmware tracks (not just regional CDNs):

| Locale | API host | Binary host | Filename locale code |
|---|---|---|---|
| English (overseas) | `api-prod.xteink.cc` | `overseas-upload-file-api.oss-ap-southeast-1.aliyuncs.com` | `EN` |
| Chinese (domestic) | `api-prod.xteink.cn` | `domestic-static-file.oss-cn-hangzhou.aliyuncs.com` | `CH` |

The domestic track has historically been ahead of the overseas track on version numbers (observed: V5.4.3 domestic vs V5.1.6 overseas). Unlocker does not need to model this; it simply serves the appropriate CrossPoint variant based on which host the device hits.

### 5.2 Request

```
GET /api/v1/check-update
    ?current_version=V5.1.0
    &device_type=ESP32C3_X3
    &device_id=<arbitrary>
    &lng=en
HTTP/1.1
Host: api-prod.xteink.cc
```

| Parameter | Values | Used by Unlocker |
|---|---|---|
| `current_version` | `V{semver}`, e.g. `V5.1.0` | Logged for diagnostics; not used for routing |
| `device_type` | `ESP32C3_X3`, `ESP32C3_X4`, or bare `ESP32C3` (defaults to X4) | **Routes to the matching CrossPoint binary** |
| `device_id` | Arbitrary string; appears not to be validated server-side | Logged only |
| `lng` | `en`, `zh` | **Cosmetic only** — host determines locale, lng is ignored by the real API |

### 5.3 Response

```json
{
  "code": 0,
  "data": {
    "version": "V5.1.6",
    "change_log": "1. Optimize EPUB\n2. Fix a large number of bugs\n...",
    "download_url": "https://overseas-upload-file-api.oss-ap-southeast-1.aliyuncs.com/.../V5.1.6-X4-EN-PROD-0304_.bin",
    "size": 6368656,
    "upload_time": "2026-03-19T01:11:49.526739+00:00",
    "checksum": null
  },
  "message": "Update available"
}
```

Notable: `checksum` is `null` in observed responses despite the schema supporting it. Stock firmware appears not to enforce integrity beyond what TLS provides. Unlocker will populate the checksum field with a real SHA-256 of its served payload as a defensive measure, but does not depend on stock behaviour.

### 5.4 Binary format

Stock firmware binaries are ESP32-C3 OTA app partition images, suitable for `esp_ota_write()`. They are *not* full-flash images — they contain only the application partition, not the bootloader or partition table. Observed sizes: ~6.3MB.

Filename convention: `V{version}-{X3|X4}-{EN|CH}-PROD-{MMDD}[_{HHMMSS}].bin`

Unlocker does not need to mimic this filename when serving — it controls both the manifest's `download_url` and the file path on the local server. However, if discovery reveals that stock firmware validates filename structure before fetching, Unlocker can trivially mimic the pattern.

### 5.5 Hardware

- **SoC:** Espressif ESP32-C3 (single-core RISC-V, integrated Wi-Fi)
- **Flash:** 16MB (per the WebSerial flasher's full-flash backup/restore feature)
- **OTA mechanism:** Standard ESP-IDF OTA flow (`esp_https_ota` or equivalent), writing to `ota_0` / `ota_1` partitions with rollback on boot failure
- **Code signing:** Disabled. `CONFIG_SECURE_SIGNED_OTA` is off in stock builds. This is the foundational assumption Unlocker depends on.

---

## 6. CrossPoint integration

### 6.1 Existing infrastructure

The crosspointreader.com Cloudflare Worker already builds, stores, and serves CrossPoint firmware across three channels:

- **Stable** — published as GitHub Releases on `crosspoint-reader/crosspoint-reader`, fetched on demand by the worker. Firmware binary streamed via `/api/release/firmware`.
- **Insider (nightly)** — built by a GitHub Actions workflow on `SoFriendly/crosspoint-tools` triggered by pushes to master. Binaries uploaded to the worker's R2 bucket. Served via `/api/build/firmware`. Metadata (commit, AI-generated summary, changelog) at `/api/build/latest`.
- **Beta** — manually uploaded by maintainers via `POST /api/beta` with a name, notes, and `.bin`. Stored in R2. Listed at `/api/beta`, individual firmware at `/api/beta/{id}/firmware`.

All three channels produce a single firmware binary that runs on both X3 and X4. The X3/X4 distinction matters only for the WebSerial flasher's full-flash partitioning — OTA installs (Unlocker's path) bypass that entirely.

### 6.2 What needs to change

A new aggregator endpoint, `GET /api/catalog`, that returns all three channels' current state in one response. Detailed in the catalog spec (`crosspointreader-com-catalog-spec.md`). Approximately one Cloudflare Worker PR.

Optional follow-ups in the same area:
- Surface SHA-256 hashes for cached firmware binaries (computed at upload time for insider and beta, fetched and cached for stable).
- Add edge caching to `/api/release/latest` and `/api/build/latest` to handle the load when Unlocker hits these on every launch.

### 6.3 Unlocker's firmware cache

On the client side, Unlocker maintains a content-addressed firmware cache at `~/Library/Application Support/XteinkUnlocker/firmware/{sha256}.bin`. Verified on every load. Downloaded blobs are written here during the firmware selection step (§4.2). Cache eviction is manual via Unlocker's settings — Unlocker does not automatically delete old firmware versions, since users may want to install the same version on multiple devices without re-downloading.

The DMG does not bundle any firmware. Reasons: keeps the DMG small (~10MB), avoids stale-firmware-in-DMG problems where a user downloads Unlocker, doesn't run it for six months, and ends up installing an outdated CrossPoint version when a current one is available. The download step is short (~6MB on a typical connection) and only happens once per version per Mac.

### 6.4 Branding and trust

Unlocker is an official CrossPoint project. It lives at github.com/crosspoint-reader/xteink-unlocker, is signed by the CrossPoint signing key, distributed from crosspointreader.com, and links back to the project's existing channels (GitHub Issues, community). It is not a third-party tool.

This matters for the trust model in §9: users are running a privileged installer that touches their network and flashes firmware. They should be running it because it's the project's tool, not because it's a randomly-attributed binary on the internet.

---

## 7. System architecture

```
┌───────────────────── Tauri app (unprivileged user process) ────────────────────┐
│                                                                                  │
│  ┌──────────────┐    IPC    ┌────────────────────────────────────────────────┐ │
│  │  React UI    │  <──────> │            Rust core                           │ │
│  │  (wizard)    │           │  ┌──────────────┐  ┌──────────────────────┐   │ │
│  └──────────────┘           │  │ Orchestrator │  │ Firmware catalog     │   │ │
│                             │  │ state machine│  │ + content-addressed  │   │ │
│                             │  └──────┬───────┘  │ cache                │   │ │
│                             │         │          └──────────────────────┘   │ │
│                             │  ┌──────▼─────────────────────────────────┐   │ │
│                             │  │ User-process subsystems                │   │ │
│                             │  │  - Catalog client (reqwest)            │   │ │
│                             │  │  - Helper RPC client                   │   │ │
│                             │  │  - Session logger                      │   │ │
│                             │  └─────────────────────┬──────────────────┘   │ │
│                             └────────────────────────┼──────────────────────┘ │
└────────────────────────────────────────────────────────┼────────────────────────┘
                                                         │ JSON-RPC over
                                                         │ Unix domain socket
┌────────────────────────────────────────────────────────▼────────────────────────┐
│              Privileged helper (runs as root via password prompt)                │
│                                                                                  │
│  System-level ops:                  Spoofing servers (bound to bridge IP):       │
│   - feth virtual upstream             - DNS (hickory) on :5353 (← pfctl 53)      │
│   - Internet Sharing NAT plist        - HTTP (axum) on :80                       │
│   - pfctl anchor (53 → 5353)          - HTTPS (axum + rustls) on :443            │
│   - dhcpd_leases polling                with self-signed cert                    │
│   - Crash-recovery state file                                                    │
└──────────────────────────────────────────────┬─────────────────────────────────┘
                                                │ Wi-Fi (bridge100)
                                                ▼
                                      ┌──────────────────┐
                                      │ Xteink X3 / X4   │
                                      │ stock firmware   │
                                      └──────────────────┘
```

The privileged helper is a separate binary launched via `osascript` with an administrator password prompt. It exposes a typed JSON-over-Unix-socket protocol with a small, enumerated surface — no arbitrary shell access. The helper owns *both* the macOS system-state operations *and* the spoofing servers themselves: ports 53/80/443 are privileged on macOS, and rather than juggle file-descriptor passing between processes, the servers run inside the helper and the unprivileged main drives them via RPC.

Privileged operations exposed by the helper:

- Creating, configuring, and destroying virtual `feth` interfaces (the synthetic upstream)
- Writing `/Library/Preferences/SystemConfiguration/com.apple.nat.plist` for Internet Sharing configuration
- Installing and removing `pfctl` anchor rules (53 → 5353)
- Reading `/var/db/dhcpd_leases`
- Reading bridge interface state via `ifconfig`
- `ArmServers` / `DisarmServers` — start/stop the DNS, HTTP, and HTTPS servers bound to the bridge IP. The unprivileged main passes in the firmware blob path and manifest contents; the helper generates a self-signed certificate for the spoofed hostname.
- `WaitManifest` / `WaitFirmware` — long-blocking RPCs that return when the helper's HTTP server sees a manifest request or completes a firmware stream. Each call uses its own Unix-socket connection so they don't queue behind other ops.

Every helper operation is reversible and tracked in `/var/db/com.sofriendly.crosspoint.unlocker.helper.state.json`. On launch, the helper reads that file and reverses anything left in place by a prior crash.

### 7.1 Orchestrator state machine

```
Idle
  │ user opens app
  ▼
Consenting
  │ user accepts both consent boxes
  ▼
SelectingDeviceAndRegion
  │ user picks model (X3/X4) and region (EN/CH)
  ▼
SelectingFirmware ──────────────────────┐
  │ user taps a channel card             │ catalog fetch fails → user sees error,
  ▼ (auto-fires download)                │ retries when online
DownloadingFirmware ────────────────────┤
  │ download complete, sha256 verified   │ download fails / hash mismatch →
  ▼                                      │ retry / cancel
SettingUpHotspot ───────────────────────┤
  │ feth created, NAT plist written      │ helper failure → Failed
  ▼                                      │
WaitingForInternetSharing ─────────────┤
  │ user enables Internet Sharing in     │ timeout (5min) → diagnostic
  │ System Settings; bridge100 appears   │
  ▼                                      │
AwaitingClient ─────────────────────────┤
  │ DHCP lease seen for Espressif MAC    │ timeout (5min) → diagnostic + retry
  ▼                                      │
AwaitingDeviceRequest ──────────────────┤
  │ on-screen instructions: tap Check    │ timeout (5min) → diagnostic
  │ for Updates. Manifest request hits   │ ("did you pick the right region?")
  │ HTTPS server.                        │
  ▼                                      │
Serving                                 │
  │ device fetches binary                │
  ▼                                      │
Flashing ───────────────────────────────┤
  │ stream completes, device reboots     │ stream interrupted →
  ▼                                      │ device may auto-rollback or brick
Verifying ──────────────────────────────┤
  │ user confirms CrossPoint is running  │ timeout (5min) → diagnostic +
  ▼                                      │ direct to recovery options
Done

  │ Cancel from any state ──────────────────► CleaningUp
                                               │ helper reverses all changes
                                               ▼
                                              Idle
```

States are exposed to the UI via Tauri events. The UI is a state-driven view — each state maps to a wizard screen. Failure transitions from any state lead to a `Failed` state with a state-specific recovery path; `CleaningUp` is invoked on Cancel or Failed and is responsible for reversing any partially-applied changes (virtual interface, Internet Sharing config, pfctl rules, installed CA on device side via user prompts).

### 7.2 Subsystem details

**Network detector.** Uses `getifaddrs(3)` via the `nix` crate plus macOS `SystemConfiguration` framework calls (via `system-configuration` crate) to enumerate interfaces and detect when `bridge100` (the AP-side bridge interface) and `feth0` (the synthetic upstream) appear or disappear. Polls every 500ms during transient states; idles otherwise. No longer concerned with detecting user-supplied upstream interfaces, since Unlocker creates its own.

**Hotspot controller.** Orchestrates the privileged helper to: (1) create the `feth` virtual upstream, (2) write the Internet Sharing config plist. The user then manually enables Internet Sharing in System Settings. Unlocker polls for `bridge100` to appear. On teardown, the helper reverses all changes. Maintains state in a small file at `~/Library/Application Support/XteinkUnlocker/hotspot-state.json` so that if Unlocker crashes, the next launch can detect leftover state and clean it up automatically.

**DNS server (in helper).** Built on `hickory-proto`. Bound to the bridge IP on port 5353 (the helper's `pfctl` anchor redirects 53 → 5353 to work around Internet Sharing's built-in DNS). For the *active* intercept hostname (`api-prod.xteink.cc` for English-region users OR `api-prod.xteink.cn` for Chinese-region users — never both), responds with the bridge IP. For all other queries, forwards upstream via `hickory-resolver` against Cloudflare 1.1.1.1. Every query is logged.

**HTTPS server (in helper).** `axum` + `axum-server` with `rustls`. Two listeners on the bridge IP: 80 and 443. Handlers:

- `GET /api/v1/check-update` — generates manifest based on Host header and `device_type` parameter; fires `on_manifest_request` notify
- `GET /firmware/{filename}` — streams the matching firmware blob with correct headers; fires `on_firmware_streamed` notify when the stream ends
- `*` — 404 with the path logged for diagnostics

The manifest handler is approximately:

```rust
async fn check_update(
    State(session): State<Arc<Session>>,
    headers: HeaderMap,
    Query(q): Query<UpdateQuery>,
) -> Result<Json<Manifest>, ApiError> {
    // Log what the device sent — useful for diagnostics, not used for routing.
    tracing::info!(
        host = ?headers.get(HOST),
        device_type = %q.device_type,
        current_version = %q.current_version,
        "stock device requested update"
    );

    // The user already picked model and region in §4.2.
    let model = session.model();    // X3 | X4
    let locale = session.locale();  // English | Chinese

    let artifact = session.selected_artifact()?;

    // Mimic Xteink's filename pattern in case stock validates URL structure (D6).
    let mimicked_filename = format!(
        "V99.9.9-{model}-{locale}-PROD-{date}.bin",
        model = model.short(),                    // "X3" / "X4"
        locale = locale.short(),                  // "EN" / "CH"
        date = chrono::Utc::now().format("%m%d"),
    );

    Ok(Json(Manifest {
        code: 0,
        data: ManifestData {
            version: "V99.9.9".to_string(),
            change_log: render_changelog(locale, model, &artifact),
            download_url: format!(
                "http://{}/firmware/{}",
                session.bridge_ip, mimicked_filename,
            ),
            size: artifact.size,
            upload_time: chrono::Utc::now().to_rfc3339(),
            checksum: Some(artifact.sha256.clone()),
        },
        message: "Update available".to_string(),
    }))
}
```

The DNS server only spoofs the one hostname matching the user's selected region (`api-prod.xteink.cc` for English, `api-prod.xteink.cn` for Chinese). The other hostname is forwarded normally. This means a wrong-region selection causes the device's request to go to real Xteink (or fail) rather than to Unlocker — which is the natural failure mode and surfaces as a 5-minute "no request received" timeout in the UI, with diagnostic copy that names the most likely cause.

The binary endpoint accepts the mimicked filename pattern and serves the same blob — the filename is for stock's benefit, not for routing:

```rust
async fn serve_firmware(
    State(session): State<Arc<Session>>,
    Path(_filename): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let artifact = session.selected_artifact()?;
    Ok(stream_file(&artifact.path).await?)
}
```

**TLS for HTTPS spoofing.** The helper generates a self-signed certificate (via `rcgen`) for the active locale's API hostname when arming servers. Stock Xteink firmware does not validate TLS certificates during OTA checks, so a self-signed cert is sufficient. No root CA or certificate installation on the device is required.

**Catalog client.** Two responsibilities:

1. **Catalog fetch.** When internet is available, fetches `https://crosspointreader.com/api/catalog` (TLS pinned to the CrossPoint project's CDN cert). Surfaces the channel data to the UI for the firmware selection screen. Failures are surfaced to the user — without a catalog, there's nothing to install. Cached for the duration of the session so the selection screen stays responsive after the initial fetch.

2. **On-demand download.** When the user picks a version, streams the firmware from the URL the catalog provided (one of `/api/release/firmware`, `/api/build/firmware`, or `/api/beta/{id}/firmware`). Computes SHA-256 on the way through and stores the file under that hash. If the catalog later exposes a `firmware_sha256` field, Unlocker compares against the expected value before declaring the download complete; until then, the SHA-256 is purely a content-address for the cache. Resume on interrupted downloads using HTTP Range requests if the worker supports them. Downloads happen during the firmware preparation step (§4.2), well before any network teardown.

The client uses `reqwest` with explicit cert pinning. Cache access uses file locks to prevent corruption if multiple Unlocker instances run concurrently (rare but possible during updates).

**Session logger.** `tracing` + `tracing-subscriber` for structured logs. During Armed/Serving/Flashing states, also runs a lightweight pcap-style capture limited to the bridge interface and the connected device's MAC. User can export the session bundle (logs + pcap + sanitized config snapshot) as a tarball for debugging or sharing.

---

## 8. Data model

### 8.1 Firmware catalog (live JSON, fetched from the worker)

Unlocker fetches `https://crosspointreader.com/api/catalog` at the channel-selection step and at app launch. Schema:

```json
{
  "schema_version": 1,
  "releases": [
    {
      "id": "stable-1.2.0",
      "channel": "stable",
      "version": "1.2.0",
      "released_at": "2026-04-15T00:00:00Z",
      "notes": "- Improved EPUB rendering\n- Custom sleep screens\n- Bug fixes",
      "firmware_url": "https://crosspointreader.com/api/release/firmware",
      "firmware_sha256": "abc123…",
      "size": 6291456
    },
    {
      "id": "beta-1.3.0-rc1",
      "channel": "beta",
      "version": "1.3.0-rc1",
      "released_at": "2026-04-25T00:00:00Z",
      "notes": "...",
      "firmware_url": "https://crosspointreader.com/api/beta/1/firmware",
      "firmware_sha256": null,
      "size": 6320000
    },
    {
      "id": "insider-latest",
      "channel": "insider",
      "version": "nightly-abc1234",
      "released_at": "2026-04-29T00:00:00Z",
      "notes": "Latest nightly build",
      "firmware_url": "https://crosspointreader.com/api/build/firmware",
      "firmware_sha256": null,
      "size": 6350000
    }
  ]
}
```

Channels are exposed as three cards on the channel-selection screen; Unlocker auto-picks the latest release on the chosen channel. There is no version-by-version picker — that's a v0.2 settings concern.

The same CrossPoint binary runs on both X3 and X4 — there is no per-model artifact split. The `model` choice in §4.2 affects only the spoofed manifest's filename pattern and the `device_type` Unlocker expects in the request.

If a release's `firmware_sha256` is non-null, Unlocker verifies the download against it. If null, the locally-computed SHA-256 is used purely as the cache key.

### 8.2 Session log entry (JSON)

```json
{
  "ts": "2026-04-29T14:30:12.345Z",
  "session_id": "uuid",
  "event": "manifest_served",
  "device_mac": "redacted",
  "device_ip": "192.168.2.5",
  "request": {
    "host": "api-prod.xteink.cc",
    "path": "/api/v1/check-update",
    "query": { "device_type": "ESP32C3_X4", "current_version": "V5.1.0", "lng": "en" }
  },
  "response": {
    "status": 200,
    "version_offered": "V99.9.9",
    "download_url": "http://192.168.2.1/firmware/crosspoint-x4-ota.bin",
    "size": 6291456
  }
}
```

---

## 9. Security and trust model

Unlocker takes privileged actions on behalf of the user. The trust model is explicit about what's at stake.

### 9.1 What Unlocker does that requires trust

- Runs as root (via the privileged helper, launched with an admin password prompt) for port binding and `pfctl` rule installation
- Streams a firmware binary that will be flashed to the user's device

### 9.2 What Unlocker does to be worthy of that trust

- **Source-available, MIT-licensed, in the CrossPoint org.** Anyone can audit.
- **Reproducible builds.** The signed binary on crosspointreader.com matches a public commit hash and CI build artifact. Users can verify.
- **Signed releases.** Unlocker is notarized by Apple (DMG signing) and additionally signed by the CrossPoint project's release signing key. Both signatures are verified at launch.
- **Firmware verified by hash.** Unlocker computes SHA-256 on every downloaded firmware blob and uses it as the cache key. If the catalog exposes an expected `firmware_sha256`, Unlocker verifies the download against it and rejects mismatches. Cache reads re-verify before use.
- **Smallest possible privileged surface.** The privileged helper exposes a small set of enumerated operations. No arbitrary shell access. No file I/O outside an explicit allowlist.
- **Teardown removes everything.** On clean exit, Unlocker removes `pfctl` rules, destroys the virtual interface, and restores the NAT plist. The user is guided to disable Internet Sharing.

### 9.3 Threat model exclusions

Unlocker does not protect against:

- A compromised Mac (root attacker can do anything, including modifying Unlocker itself)
- A compromised CrossPoint signing key (would allow malicious firmware to be served via legitimate-looking releases)
- Network-level attackers between Unlocker and GitHub (defended by HTTPS + cert pinning, but not Unlocker's primary concern)
- Supply chain attacks on Unlocker's dependencies (mitigated by `cargo audit`, dependency pinning, and reproducible builds, but not eliminated)

---

## 10. Failure modes

Each is a first-class UI state with copy written for a non-technical user.

| Failure | Detection | UI state | Recovery |
|---|---|---|---|
| Catalog endpoint unreachable | HTTP error or timeout fetching `/api/catalog` | "Couldn't reach crosspointreader.com. Check your internet connection." with retry button. | User reconnects; retry |
| Firmware download interrupted | Stream closes before `Content-Length` bytes sent | "Download interrupted. Resume?" with Resume / Retry / Cancel buttons | Auto-resume via HTTP Range; manual retry available |
| Firmware SHA-256 mismatch | Hash check fails after download | "The firmware download appears damaged. This could be a network issue or a security concern." with retry and report-issue buttons | Re-download; if persistent, raise a project-level alert |
| Virtual interface creation fails | `feth0` doesn't appear after privileged helper call | "Couldn't set up the local network. This sometimes happens on macOS [version]. Trying alternative method..." → falls back to custom AP setup if implemented, or to phone-tether mode with explicit instructions | Auto-fallback; manual mode as last resort |
| Internet Sharing not enabled | `bridge100` doesn't appear within 5 min of showing instructions | "Internet Sharing didn't start. Make sure you selected feth7 as the source and Wi-Fi as the destination in System Settings." with troubleshooting steps | User follows instructions; auto-progresses when bridge100 appears |
| Wi-Fi already disabled | Wi-Fi power state off when hotspot setup begins | "Please turn Wi-Fi on. Unlocker needs Wi-Fi to create the local network for your device." | User turns Wi-Fi on; auto-progresses |
| Device never appears on bridge | No DHCP lease within 5 min of hotspot up | "Is your Xteink connected to the SSID?" with on-device steps to verify | User reconnects device; auto-progresses |
| Device never sends check-update | No request received within 5 min of arming | "Tap Settings → System → Check for Updates on your Xteink" with screenshots | User triggers check; auto-progresses |
| Stock firmware sends unknown request | Catch-all handler hit during Armed state | "Your Xteink is using a newer protocol than Unlocker knows about" with link to file an issue and the captured request payload | User reports; Unlocker cannot proceed |
| Download interrupted | Stream closes before `Content-Length` bytes sent | "The download was interrupted. Your device may be in a partial state." with recovery instructions | If USB works: WebSerial full-flash restore. If USB is locked: device may auto-rollback via OTA partition A/B; if not, UART recovery is the only path |
| Device doesn't reboot into CrossPoint | Verifying state times out at 5 min | "Verification failed" with diagnostic capture | Same as above. Unlocker's diagnostic bundle is critical here for community debugging |
| App crashes mid-session | Detected on next launch via stale state file | "Unlocker didn't shut down cleanly. Cleaning up..." | Auto-removes pfctl rules, stops orphan helper, returns to Idle |

---

## 11. Tech stack

### 11.1 Frontend

- **Tauri 2.x** — desktop wrapper, IPC, packaging, code signing
- **TypeScript + React** — UI
- **Tailwind CSS** — styling, with a design system that matches crosspointreader.com aesthetic (clean, restrained, e-ink-inspired)
- **Zustand or similar** — UI state (driven by Tauri events from the orchestrator)

### 11.2 Backend (Rust core)

- **`tokio`** — async runtime
- **`hickory-dns`** — DNS server
- **`axum` + `rustls`** — HTTP/HTTPS server
- **`rcgen`** — self-signed certificate generation for HTTPS spoofing
- **`reqwest`** — outbound HTTPS for catalog fetching
- **`tracing` + `tracing-subscriber`** — structured logging
- **`serde` + `toml`** — configuration

### 11.3 Privileged helper

- Separate Rust binary
- Communicates with main process via Unix domain socket
- Typed JSON-RPC protocol
- Launched as root via `osascript` with admin password prompt

### 11.4 Build and release

- Cargo workspace with three crates: `unlocker-core`, `unlocker-helper`, `unlocker-tauri`
- GitHub Actions CI: build, test, sign, notarize, release
- Reproducible builds via fixed Rust toolchain version and `Cargo.lock` commit
- Distribution: notarized DMG, hosted at `crosspointreader.com/unlocker`

---

## 12. Distribution and lifecycle

### 12.1 Distribution

- Notarized DMG, signed with Apple Developer ID and CrossPoint signing key (~10MB — firmware is downloaded on demand, not bundled)
- Hosted directly from `crosspointreader.com/unlocker`
- Linked from the existing flash page as: "No USB cable? Try Xteink Unlocker (macOS, beta)"
- **Not distributed via the Mac App Store.** Privileged network interception will not pass App Store review, and asking it to is the wrong fight.

### 12.2 Updates

- Tauri's built-in update mechanism, signed update manifests
- Two update paths run in parallel:
  - **Unlocker app updates** (Tauri auto-update). Major fixes, new features, support for new Xteink firmware versions.
  - **Firmware-only updates** (catalog refresh). When CrossPoint releases a new version (stable, beta, or insider), it appears in `/api/catalog` automatically. Existing Unlocker installations see the new version on next launch.
- New CrossPoint firmware does not require a new Unlocker release.

### 12.3 Telemetry

**None by default.** The user threat model includes "I don't want a project I'm trusting with privileged access reporting back about me." Unlocker ships with telemetry off and no opt-in in v1.

In v2, an opt-in "Help improve Unlocker" toggle could enable session bundle uploads (logs + sanitized configs) for diagnostic purposes. Off by default. Clearly labelled. Reviewable before send.

---

## 13. Discovery tasks before build

These are empirical questions that gate or shape implementation. Each is a small, scoped test.

| ID | Question | Test | Decision impact |
|---|---|---|---|
| D1 | Is `ESP32C3_X4` the form X4 sends? | `curl '...?device_type=ESP32C3_X4&...'` against both hosts | Confirms catalog routing |
| D2 | Does `lng` truly have no routing effect? | Cross-product of hosts × lng values | Confirms Unlocker can ignore lng for routing |
| D5 | Does stock firmware fall back to HTTP if HTTPS fails? | Block HTTPS, allow HTTP at the same hostname, observe | Determines fallback availability |
| D6 | Does stock firmware validate `download_url` host or scheme? | Substitute manifest with HTTP URL on a different host, observe | Determines whether Unlocker needs to mimic Aliyun URLs |
| D7 | Does stock firmware enforce non-null checksum? | Manifest with deliberately-wrong checksum, observe | Confirms checksum field handling |
| D8 | Does CrossPoint use runtime locale or build-time locale? | Read CrossPoint source / ask maintainers | Determines artifact count (2 vs 4) |
| D9 | What's the X3 vs X4 USB OTA layout? | Read existing WebSerial flasher source | Sanity check for OTA partition assumptions |
| D10 | Is there a recovery partition? | Inspect partition table from a flash backup | Informs failure-mode messaging |
| D11 | Does Internet Sharing accept `feth` as upstream on macOS 14+? | **Settled.** `feth` is accepted as upstream on macOS 14–26, but `launchctl kickstart` is no longer reliable for starting the service programmatically. Solution: user manually enables Internet Sharing in System Settings. Unlocker writes the NAT plist and polls for `bridge100`. | N/A — resolved |
| D12 | What's the actual size of the stock OTA `.bin` files vs. the OTA partition size? | Inspect partition table from a flash backup; compare to the 6.3MB observed binary size | Confirms there's headroom for CrossPoint's binary, which may be larger or smaller than stock |

---

## 14. Adversarial dynamics

Unlocker exists because Xteink took an action against custom-firmware installation. It would be naïve to assume that's the last action they'll take. This section enumerates the moves Xteink could plausibly make next, the impact of each, and Unlocker's response.

### 14.1 OTA payload signing

**Move:** Xteink enables `CONFIG_SECURE_SIGNED_OTA` and starts signing OTA payloads with a private key whose public counterpart is baked into the bootloader.

**Impact:** Unlocker can no longer substitute payloads. The mechanism is fundamentally broken for any device shipped with the signed firmware.

**Response:** Detection during discovery — the bootloader rejects unsigned binaries at install time. Unlocker's catalog flags affected versions. There is no workaround short of key extraction or exploit chaining, both out of scope.

This is the move that ends Unlocker as a viable approach. It's also the move with the highest engineering cost for Xteink — they have to set up signing infrastructure, manage keys, and accept that any signing-key compromise becomes a fleet-wide vulnerability. The fact that they haven't done this *and* they shipped a USB lockdown suggests they prefer cheap blunt instruments over expensive correct ones, which is the situation Unlocker exploits.

### 14.2 API hostname rotation

**Move:** Xteink starts pushing OTA updates that change the hardcoded hostname in the firmware to something Unlocker doesn't intercept.

**Impact:** Existing Unlocker installations stop working until updated.

**Response:** Trivial. Unlocker's intercept hostnames are config, not code. A point release with the new hostname ships in hours, not weeks. Users update Unlocker, intercept resumes.

### 14.3 Manifest schema changes

**Move:** Xteink changes the `check-update` response schema (e.g., adds required signed fields, changes the version comparison logic, adds a nonce/challenge).

**Impact:** Unlocker's manifest server returns responses the new firmware rejects. Devices on the new firmware can't be intercepted.

**Response:** Discovery probe captures the new schema, catalog ships an updated manifest template, and Unlocker accommodates. Slow but tractable. Users on already-updated stock firmware have to wait for an Unlocker release.

### 14.4 OTA disabled entirely

**Move:** Xteink removes the in-firmware OTA mechanism, requires updates via their own app or USB-to-something-proprietary.

**Impact:** Total. Unlocker has nothing to intercept.

**Response:** Project pivots or shuts down the Unlocker path. CrossPoint as a project continues for older devices, but new shipments become permanently uninstallable.

### 14.5 Implications for Unlocker's design

The moves above suggest several design priorities that are already in the spec but worth calling out explicitly:

- **Fast-config, slow-code.** Anything Xteink can change cheaply (hostnames, schema fields, version numbers) should be config in Unlocker, not code. This minimises response time to counter-updates.
- **Explicit firmware version compatibility matrix.** Users with firmware version X see "confirmed working", "confirmed broken", or "untested" — never silent failure. This builds trust and surfaces breakage early.
- **Community reporting loop.** When a device fails, the diagnostic bundle should be one click away from a pre-filled GitHub issue. The faster the project sees breakage, the faster the catalog gets updated.
- **Honest non-promises.** The spec should never claim Unlocker will work on "all Xteink X3/X4 devices forever." It works on the firmware versions in the compatibility matrix, and may stop working on future versions.

### 14.6 Legal and ethical note

Unlocker installs user-chosen firmware on devices the user owns. This is a clearly established right in most jurisdictions. However, Xteink may choose to:

- Cite trademark or terms-of-service violations in a takedown attempt
- Block accounts associated with detected Unlocker usage (the `device_id` parameter in OTA requests could be used for fingerprinting)
- Brick devices via stock OTA that detect a CrossPoint installation history (hard but not impossible)

The CrossPoint project should have a legal posture prepared. Recommended posture: Unlocker only intercepts requests on the user's local network, doesn't violate any anti-circumvention provisions because there is no DRM being circumvented, and modifies only firmware on a device the user owns. If Xteink files a takedown, the project responds with counter-notices and continues. The DMCA Section 1201 exemption for "computer programs that enable wireless devices to connect to a wireless telecommunications network" is potentially relevant and worth a real lawyer's read.

---

## 15. Roadmap

### v0.1 — Spec and skeleton (current)

- This document
- Cargo workspace: `unlocker-core`, `unlocker-helper`, Tauri app
- Privileged helper with real macOS shell-outs (`feth` create/destroy via `ifconfig`, Internet Sharing via NAT plist, `pfctl` anchor for DNS port redirect, `dhcpd_leases` parsing, crash-recovery state file)
- DNS / HTTP / HTTPS spoofing servers running inside the helper, bound to the bridge IP, with self-signed TLS cert
- Helper launched via `osascript` admin password prompt (replaced SMAppService/LaunchDaemon approach due to provisioning profile requirements on macOS 26)
- React + Tailwind wizard: Consent → Device + Region → Channel → Connect (hotspot setup + manual Internet Sharing enable + tap-Check) → Install progress → Verify → Done
- `cargo check --workspace` clean on latest crate versions; helper builds release; frontend builds via Vite

### v0.2 — End-to-end on real hardware

- First successful install of CrossPoint on a stock Xteink X3 or X4 by an engineer
- Spec revised based on findings (manifest schema variations, filename validation, TLS cert validation behaviour)
- Failure-mode coverage per §10 (named recovery screens with state-specific copy + timeouts)
- `~/Library/Application Support/XteinkUnlocker/hotspot-state.json` for crash recovery on the main side too (helper already has its own)

### v0.3 — Alpha (CrossPoint maintainers)

- Full wizard polish, edge cases shaken out
- Distributed as signed DMG to maintainers for testing across X3 / X4 / EN / CH
- Tauri auto-update wired up

### v0.4 — Public beta

- Notarized, signed DMG
- Linked from the flash page
- Documentation on crosspointreader.com

### v1.0 — General availability

- Stable for 4+ weeks of beta with no critical issues
- Telemetry decision finalized (likely staying off-by-default)
- Issue triage process established

### Post-v1

- **Linux support.** `hostapd` + `dnsmasq` orchestration. The privileged helper story is messier than macOS but tractable.
- **Windows support.** Mobile Hotspot is finicky; the privileged-helper story is the worst of the three platforms. Lowest priority.
- **Multi-device queue.** For the "Reviewer / Recommender" persona — flash 5 devices in sequence without re-running the wizard each time.
- **CrossPoint → CrossPoint updates.** If CrossPoint adds its own OTA mechanism, Unlocker could become unnecessary for upgrades — but it remains the install path for fresh-from-stock devices.
- **Settings screen.** Manual cache eviction, version-by-version firmware picker for power users, opt-in telemetry toggle.

---

## 16. Open questions for CrossPoint maintainers

1. **Release pipeline ownership.** Who owns adding the OTA-flavoured artifacts to the CrossPoint CI? Unlocker is blocked on this artifact existing for every release.
2. **Runtime vs build-time locale.** Is CrossPoint's UI language picked at runtime or compiled in? Determines artifact count.
3. **Versioning relationship.** Should Unlocker versions track CrossPoint versions, be independent, or use a hybrid (e.g., Unlocker 1.x always supports CrossPoint 1.x)?
4. **Repo location.** Should Xteink Unlocker live at `crosspoint-reader/xteink-unlocker` (new repo) or as a subdirectory of the main CrossPoint repo? New repo is probably right — different release cadence, different platform constraints, separate issue tracker — but worth a deliberate decision.
5. **Recovery responsibility.** Xteink Unlocker directs users to the WebSerial full-flash restore for recovery. Is that path well-documented enough to send users to it confidently? If not, that's a docs task on the main project.

---

## Appendix A: Example intercepted manifest response

What Unlocker serves to a stock X4 device with `lng=en` connecting to `api-prod.xteink.cc`:

```json
{
  "code": 0,
  "data": {
    "version": "V99.9.9",
    "change_log": "Installing CrossPoint Reader 1.2.0\n\nThis update replaces the stock Xteink firmware with CrossPoint, an open-source firmware with more features and full local control.\n\nCrossPoint 1.2.0 highlights:\n- EPUB 2 and 3 rendering with embedded CSS\n- Three built-in font families plus custom font support\n- KOReader Sync\n- Calibre plugin for wireless transfers\n- MIT licensed, source on GitHub\n\nLearn more: https://crosspointreader.com\n\nIf you change your mind, you can restore stock firmware via the WebSerial flasher.",
    "download_url": "http://192.168.2.1/firmware/crosspoint-x4-ota.bin",
    "size": 6291456,
    "upload_time": "2026-04-29T14:30:00.000000+00:00",
    "checksum": "sha256:abc123def456..."
  },
  "message": "Update available"
}
```

The `change_log` is the user's last off-ramp before the install proceeds. It must be honest, non-coercive, and offer a clear way out.

## Appendix B: Glossary

- **OTA** — Over-the-air, firmware update delivered via network rather than USB
- **Bridge interface** — `bridge100`-style virtual interface created by macOS Internet Sharing
- **Stock firmware** — Xteink's factory firmware, the thing Unlocker replaces
- **CrossPoint** — Open-source replacement firmware, the thing Unlocker installs
- **WebSerial** — Browser API for serial port access, used by the existing CrossPoint flasher
- **ESP-IDF** — Espressif's IoT Development Framework, the SDK CrossPoint and stock are built against
- **`pfctl`** — macOS packet filter control, used here for port redirection
- **`bridge100`** — The default interface name macOS gives to the Internet Sharing bridge

---

*End of spec, v0.1.*
