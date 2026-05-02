#!/usr/bin/env bash
# Push the latest macOS build artifacts to R2, then merge a fresh entry into
# latest.json so the Tauri updater picks it up. Run after build-macos.sh.
#
# Required env (loaded from .env.local if present):
#   CLOUDFLARE_ACCOUNT_ID
#   CLOUDFLARE_R2_ACCESS_KEY  (or AWS_ACCESS_KEY_ID)
#   CLOUDFLARE_R2_SECRET_KEY  (or AWS_SECRET_ACCESS_KEY)
#
# Optional:
#   CLOUDFLARE_R2_BUCKET   defaults to "unlocker-releases"

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

if [[ -f .env.local ]]; then
    # shellcheck disable=SC2046
    export $(grep -v '^#' .env.local | xargs)
fi

[[ -z "${CLOUDFLARE_R2_ACCESS_KEY:-}" ]] && CLOUDFLARE_R2_ACCESS_KEY="${AWS_ACCESS_KEY_ID:-}"
[[ -z "${CLOUDFLARE_R2_SECRET_KEY:-}" ]] && CLOUDFLARE_R2_SECRET_KEY="${AWS_SECRET_ACCESS_KEY:-}"

: "${CLOUDFLARE_ACCOUNT_ID:?CLOUDFLARE_ACCOUNT_ID not set}"
: "${CLOUDFLARE_R2_ACCESS_KEY:?CLOUDFLARE_R2_ACCESS_KEY not set}"
: "${CLOUDFLARE_R2_SECRET_KEY:?CLOUDFLARE_R2_SECRET_KEY not set}"

CLOUDFLARE_R2_BUCKET="${CLOUDFLARE_R2_BUCKET:-unlocker-releases}"
R2_ENDPOINT="https://${CLOUDFLARE_ACCOUNT_ID}.r2.cloudflarestorage.com"

VERSION=$(grep '"version"' app/src-tauri/tauri.conf.json | head -1 | sed 's/.*"version": "\(.*\)".*/\1/')
echo "Uploading version: $VERSION"

# ── Extract release notes from CHANGELOG.md (if present) ──
extract_changelog() {
  local version=$1
  local changelog_file="CHANGELOG.md"
  [[ -f "$changelog_file" ]] || { echo "Update to version ${version}"; return; }

  local notes
  notes=$(awk -v ver="$version" '
    /^## \[/ {
      if (found) exit
      if ($0 ~ "\\[" ver "\\]") found=1
      next
    }
    found && !/^## / { print }
  ' "$changelog_file" | sed '/^$/d' | sed 's/^- /• /')

  if [[ -z "$notes" ]]; then
    notes=$(awk '
      /^## \[/ {
        if (found) exit
        found=1
        next
      }
      found && !/^## / { print }
    ' "$changelog_file" | sed '/^$/d' | sed 's/^- /• /')
  fi
  echo "$notes"
}

CHANGELOG_NOTES=$(extract_changelog "$VERSION")
[[ -z "$CHANGELOG_NOTES" ]] && CHANGELOG_NOTES="Update to version ${VERSION}"
echo "Changelog notes:"
echo "$CHANGELOG_NOTES"

upload_file() {
  local file=$1
  local key=$2
  if [[ -f "$file" ]]; then
    echo "Uploading: $key"
    AWS_ACCESS_KEY_ID="$CLOUDFLARE_R2_ACCESS_KEY" \
    AWS_SECRET_ACCESS_KEY="$CLOUDFLARE_R2_SECRET_KEY" \
    aws s3 cp "$file" "s3://${CLOUDFLARE_R2_BUCKET}/${key}" \
      --endpoint-url "$R2_ENDPOINT" \
      --no-progress
  else
    echo "Skipping (not found): $file"
  fi
}

echo
echo "=== Uploading macOS artifacts ==="

DMG_FILE=$(find target/release/bundle/dmg -name "*.dmg" 2>/dev/null | head -1)
[[ -n "${DMG_FILE:-}" ]] && upload_file "$DMG_FILE" "v${VERSION}/XteinkUnlocker_${VERSION}_aarch64.dmg"

TAR_FILE="target/release/bundle/XteinkUnlocker_${VERSION}_darwin-aarch64.app.tar.gz"
if [[ -f "$TAR_FILE" ]]; then
  upload_file "$TAR_FILE" "v${VERSION}/XteinkUnlocker_${VERSION}_darwin-aarch64.app.tar.gz"
  [[ -f "${TAR_FILE}.sig" ]] && upload_file "${TAR_FILE}.sig" "v${VERSION}/XteinkUnlocker_${VERSION}_darwin-aarch64.app.tar.gz.sig"
else
  echo "Warning: $TAR_FILE not found — run build-macos.sh first" >&2
fi

echo
echo "=== Updating latest.json ==="

MAC_SIG=""
[[ -f "${TAR_FILE}.sig" ]] && MAC_SIG=$(cat "${TAR_FILE}.sig")

LATEST_JSON="target/release/bundle/latest.json"

# Pull existing latest.json so we preserve other platforms (none in v0.1, but
# future-proof). On first run we just bootstrap an empty document.
echo "Fetching existing latest.json…"
AWS_ACCESS_KEY_ID="$CLOUDFLARE_R2_ACCESS_KEY" \
AWS_SECRET_ACCESS_KEY="$CLOUDFLARE_R2_SECRET_KEY" \
aws s3 cp "s3://${CLOUDFLARE_R2_BUCKET}/latest.json" "$LATEST_JSON" \
  --endpoint-url "$R2_ENDPOINT" --no-progress 2>/dev/null \
  || echo '{"platforms":{}}' > "$LATEST_JSON"

PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

if [[ -n "$MAC_SIG" ]]; then
  jq --arg ver "$VERSION" \
     --arg notes "$CHANGELOG_NOTES" \
     --arg pub_date "$PUB_DATE" \
     --arg mac_sig "$MAC_SIG" \
     '.version = $ver
      | .notes = $notes
      | .pub_date = $pub_date
      | .platforms["darwin-aarch64"] = {
          "signature": $mac_sig,
          "url": ("https://unlocker-releases.crosspointreader.com/v" + $ver +
                  "/XteinkUnlocker_" + $ver + "_darwin-aarch64.app.tar.gz")
        }' "$LATEST_JSON" > "${LATEST_JSON}.tmp" \
    && mv "${LATEST_JSON}.tmp" "$LATEST_JSON"

  upload_file "$LATEST_JSON" "latest.json"
else
  echo "No macOS signature; latest.json not updated." >&2
fi

echo
echo "=== Upload complete ==="
echo "Update endpoint: https://unlocker-releases.crosspointreader.com/latest.json"
