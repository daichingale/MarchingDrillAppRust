#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 1 ]] || { echo "usage: $0 DIST.dmg" >&2; exit 64; }
DMG="$1"
[[ -f "$DMG" ]] || { echo "artifact not found: $DMG" >&2; exit 66; }
: "${DRILLFORGE_NOTARY_KEY_B64:?missing notarization API key}"
: "${DRILLFORGE_NOTARY_KEY_ID:?missing notarization key id}"
: "${DRILLFORGE_NOTARY_ISSUER_ID:?missing notarization issuer id}"
TMP_KEY="$(mktemp "${TMPDIR:-/tmp}/drillforge-notary.XXXXXX.p8")"
cleanup() { rm -f "$TMP_KEY"; }
trap cleanup EXIT INT TERM
umask 077
printf '%s' "$DRILLFORGE_NOTARY_KEY_B64" | base64 -D > "$TMP_KEY"
xcrun notarytool submit "$DMG" --key "$TMP_KEY" --key-id "$DRILLFORGE_NOTARY_KEY_ID" --issuer "$DRILLFORGE_NOTARY_ISSUER_ID" --wait
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature -v "$DMG"
shasum -a 256 "$DMG" > "$DMG.sha256"
