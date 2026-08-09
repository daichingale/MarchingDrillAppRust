#!/usr/bin/env bash
set -euo pipefail
[[ $# -ge 1 ]] || { echo "usage: $0 APP [--release]" >&2; exit 64; }
APP="$1"; MODE="${2:-}"
[[ -x "$APP/Contents/MacOS/drill-app" ]]
plutil -lint "$APP/Contents/Info.plist"
[[ "$(plutil -extract CFBundlePackageType raw "$APP/Contents/Info.plist")" == APPL ]]
VERSION="$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist")"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ -s "$APP/Contents/Resources/AppIcon.icns" ]]
for file in LICENSE LICENSE-APACHE LICENSE-MIT THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.md OFL-NotoSansJP.txt; do [[ -s "$APP/Contents/Resources/$file" ]]; done
if [[ "$MODE" == --release ]]; then
  codesign --verify --deep --strict --verbose=2 "$APP"
  spctl --assess --type execute -v "$APP"
fi
echo "macOS bundle structure passed ($VERSION)"
