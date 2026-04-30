#!/usr/bin/env bash
# Full macOS release build for Xteink Unlocker.
#
#  1. tauri build      — produces the .app and .dmg
#  2. inject helper    — copies unlocker-helper into Contents/MacOS/
#                        and the LaunchDaemon plist into Contents/Library/LaunchDaemons/
#  3. re-sign          — codesigns the helper, then re-signs the bundle so
#                        SMAppService sees a consistent signature
#  4. notarize         — submits .app and .dmg to Apple, staples on success
#  5. update bundle    — produces a signed tar.gz for Tauri auto-update
#
# Usage:
#   ./scripts/build-macos.sh [major|minor|patch]
#
# If a bump argument is provided, version is bumped first via cargo + tauri.conf.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

# ── Load .env.local if present ──
if [[ -f .env.local ]]; then
    # shellcheck disable=SC2046
    export $(grep -v '^#' .env.local | xargs)
fi

# ── Required env ──
: "${APPLE_ID:?APPLE_ID not set (put it in .env.local)}"
: "${APPLE_PASSWORD:?APPLE_PASSWORD not set (app-specific password)}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID not set}"

APPLE_CERTIFICATE_IDENTITY="${APPLE_CERTIFICATE_IDENTITY:-Developer ID Application: SoFriendly LLC (${APPLE_TEAM_ID})}"
export APPLE_CERTIFICATE_IDENTITY
export APPLE_SIGNING_IDENTITY="${APPLE_CERTIFICATE_IDENTITY}"

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    echo "warning: TAURI_SIGNING_PRIVATE_KEY not set — auto-update bundle won't be signed"
fi

echo "==> Identity: ${APPLE_CERTIFICATE_IDENTITY}"

# ── Optional: bump version first ──
if [[ -n "${1:-}" ]]; then
    echo "==> Bumping version (${1})"
    if [[ -x "${REPO_ROOT}/scripts/bump-version.sh" ]]; then
        ./scripts/bump-version.sh "$1"
    else
        echo "warning: scripts/bump-version.sh not found; skipping bump" >&2
    fi
fi

# ── Build helper first so we can inject it post-bundle ──
echo "==> Building unlocker-helper (release)"
cargo build --release -p unlocker-helper

HELPER_BIN_SRC="${REPO_ROOT}/target/release/unlocker-helper"
[[ -x "${HELPER_BIN_SRC}" ]] || { echo "helper binary missing: ${HELPER_BIN_SRC}" >&2; exit 1; }

# ── Build the Tauri app ──
echo "==> Building Tauri app"
( cd app && npm run tauri build )

# Locate the produced .app / .dmg.
APP_PATH=$(find target/release/bundle/macos -name "*.app" -type d 2>/dev/null | head -1)
DMG_PATH=$(find target/release/bundle/dmg -name "*.dmg" 2>/dev/null | head -1)

[[ -d "${APP_PATH}" ]] || { echo "no .app produced by tauri build" >&2; exit 1; }
echo "==> App bundle: ${APP_PATH}"

# ── Inject helper binary ──
echo "==> Injecting helper into app bundle"
HELPER_BIN_DST="${APP_PATH}/Contents/MacOS/unlocker-helper"

cp -f "${HELPER_BIN_SRC}" "${HELPER_BIN_DST}"
chmod 0755 "${HELPER_BIN_DST}"

echo "==> Signing helper binary"
codesign --force \
    --options runtime \
    --timestamp \
    --entitlements app/src-tauri/helper-entitlements.plist \
    --sign "${APPLE_CERTIFICATE_IDENTITY}" \
    "${HELPER_BIN_DST}"

echo "==> Re-signing app bundle"
codesign --remove-signature "${APP_PATH}" || true
codesign --force \
    --options runtime \
    --timestamp \
    --deep \
    --entitlements app/src-tauri/entitlements.plist \
    --sign "${APPLE_CERTIFICATE_IDENTITY}" \
    "${APP_PATH}"
codesign --verify --strict --deep --verbose=2 "${APP_PATH}"

# ── Notarize the .app ──
echo "==> Notarizing app"
APP_ZIP="target/release/bundle/Unlocker.zip"
ditto -c -k --keepParent "${APP_PATH}" "${APP_ZIP}"
xcrun notarytool submit "${APP_ZIP}" \
    --apple-id "${APPLE_ID}" \
    --password "${APPLE_PASSWORD}" \
    --team-id "${APPLE_TEAM_ID}" \
    --wait
xcrun stapler staple "${APP_PATH}"
rm -f "${APP_ZIP}"

# ── Notarize the DMG ──
if [[ -n "${DMG_PATH}" && -f "${DMG_PATH}" ]]; then
    echo "==> Signing + notarizing DMG"
    codesign --force --sign "${APPLE_CERTIFICATE_IDENTITY}" "${DMG_PATH}"
    xcrun notarytool submit "${DMG_PATH}" \
        --apple-id "${APPLE_ID}" \
        --password "${APPLE_PASSWORD}" \
        --team-id "${APPLE_TEAM_ID}" \
        --wait
    xcrun stapler staple "${DMG_PATH}"
fi

# ── Update bundle for Tauri auto-update ──
VERSION=$(grep '"version"' app/src-tauri/tauri.conf.json | head -1 | sed 's/.*"version": "\(.*\)".*/\1/')
TAR_FILE="target/release/bundle/XteinkUnlocker_${VERSION}_darwin-aarch64.app.tar.gz"
echo "==> Creating update bundle ${TAR_FILE}"
COPYFILE_DISABLE=1 tar -czf "${TAR_FILE}" -C "$(dirname "${APP_PATH}")" "$(basename "${APP_PATH}")"

if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    echo "==> Signing update bundle"
    if [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
        ( cd app && npx tauri signer sign --private-key "${TAURI_SIGNING_PRIVATE_KEY}" --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD}" "../${TAR_FILE}" )
    else
        ( cd app && npx tauri signer sign --private-key "${TAURI_SIGNING_PRIVATE_KEY}" "../${TAR_FILE}" )
    fi
fi

echo
echo "Build complete."
echo "  App: ${APP_PATH}"
[[ -n "${DMG_PATH}" ]] && echo "  DMG: ${DMG_PATH}"
echo "  Update bundle: ${TAR_FILE}"
