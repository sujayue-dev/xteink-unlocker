#!/usr/bin/env bash
# Dev build — signs but skips notarization for faster iteration.
# The resulting .app works locally (right-click > Open to bypass Gatekeeper).
#
# Usage:
#   ./scripts/build-macos-dev.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

# ── Load .env.local if present ──
if [[ -f .env.local ]]; then
    # shellcheck disable=SC2046
    export $(grep -v '^#' .env.local | xargs)
fi

# ── Required env ──
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID not set (put it in .env.local)}"

APPLE_CERTIFICATE_IDENTITY="${APPLE_CERTIFICATE_IDENTITY:-Developer ID Application: SoFriendly LLC (${APPLE_TEAM_ID})}"
export APPLE_CERTIFICATE_IDENTITY
export APPLE_SIGNING_IDENTITY="${APPLE_CERTIFICATE_IDENTITY}"

echo "==> Identity: ${APPLE_CERTIFICATE_IDENTITY}"

# ── Build helper ──
echo "==> Building unlocker-helper (release)"
cargo build --release -p unlocker-helper

HELPER_BIN_SRC="${REPO_ROOT}/target/release/unlocker-helper"
[[ -x "${HELPER_BIN_SRC}" ]] || { echo "helper binary missing: ${HELPER_BIN_SRC}" >&2; exit 1; }

# ── Build the Tauri app (unset notarization env so Tauri skips it) ──
echo "==> Building Tauri app (no notarization)"
( cd app && unset APPLE_ID APPLE_PASSWORD APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER && npm run tauri build )

# Locate the produced .app.
APP_PATH=$(find target/release/bundle/macos -name "*.app" -type d 2>/dev/null | head -1)
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

echo
echo "Dev build complete (not notarized)."
echo "  App: ${APP_PATH}"
echo "  Tip: right-click > Open to bypass Gatekeeper on first launch."
