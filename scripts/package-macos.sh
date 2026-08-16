#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-dev}"
case "$MODE" in dev|release) ;; *) echo "usage: $0 [dev|release]" >&2; exit 64;; esac
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' Cargo.toml | head -1)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid workspace version" >&2; exit 65; }
if [[ -n "${GITHUB_REF_NAME:-}" && "${GITHUB_REF_TYPE:-}" == tag && "$GITHUB_REF_NAME" != "v$VERSION" ]]; then
  echo "release tag $GITHUB_REF_NAME does not match Cargo version v$VERSION" >&2; exit 65
fi
BUILD_NUMBER="${DRILLFORGE_BUILD_NUMBER:-${GITHUB_RUN_NUMBER:-1}}"
[[ "$BUILD_NUMBER" =~ ^[1-9][0-9]*$ ]] || { echo "build number must be a positive integer" >&2; exit 65; }
BUNDLE_ID="${DRILLFORGE_MACOS_BUNDLE_ID:-com.drillforge.app}"
[[ "$BUNDLE_ID" =~ ^[A-Za-z0-9][A-Za-z0-9.-]+$ && "$BUNDLE_ID" == *.* ]] || { echo "invalid bundle identifier" >&2; exit 65; }

ARCH="$(uname -m)"
case "$ARCH" in arm64) TARGET=aarch64-apple-darwin;; x86_64) TARGET=x86_64-apple-darwin;; *) echo "unsupported macOS architecture: $ARCH" >&2; exit 69;; esac
if [[ "${DRILLFORGE_MACOS_UNIVERSAL:-0}" == 1 ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --locked --release --target aarch64-apple-darwin -p drill-app
  cargo build --locked --release --target x86_64-apple-darwin -p drill-app
  OUTPUT_ARCH=universal
else
  cargo build --locked --release --target "$TARGET" -p drill-app
  OUTPUT_ARCH="$ARCH"
fi

STAGE="target/package-macos"
APP="$STAGE/DrillForge.app"
DIST="dist"
rm -rf "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$DIST"
if [[ "$OUTPUT_ARCH" == universal ]]; then
  lipo -create target/aarch64-apple-darwin/release/drill-app target/x86_64-apple-darwin/release/drill-app -output "$APP/Contents/MacOS/drill-app"
  chmod 755 "$APP/Contents/MacOS/drill-app"
  [[ "$(lipo -archs "$APP/Contents/MacOS/drill-app")" == *arm64* && "$(lipo -archs "$APP/Contents/MacOS/drill-app")" == *x86_64* ]]
else
  install -m 755 "target/$TARGET/release/drill-app" "$APP/Contents/MacOS/drill-app"
fi
sed -e "s/__VERSION__/$VERSION/g" -e "s/__BUILD_NUMBER__/$BUILD_NUMBER/g" -e "s/__BUNDLE_ID__/$BUNDLE_ID/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp LICENSE LICENSE-APACHE LICENSE-MIT THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.md assets/OFL-NotoSansJP.txt "$APP/Contents/Resources/"

ICON_SOURCE="${DRILLFORGE_ICON_PNG:-assets/app-icon.svg}"
ICONSET="$STAGE/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -s format png -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -s format png -z "$double" "$double" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

if [[ "$MODE" == release ]]; then
  : "${DRILLFORGE_MACOS_SIGN_IDENTITY:?release requires DRILLFORGE_MACOS_SIGN_IDENTITY}"
  codesign --force --deep --options runtime --timestamp --entitlements packaging/macos/DrillForge.entitlements --sign "$DRILLFORGE_MACOS_SIGN_IDENTITY" "$APP"
  codesign --verify --deep --strict --verbose=2 "$APP"
else
  echo "Creating explicitly unsigned development package"
fi

DMG="$DIST/DrillForge-$VERSION-macos-$OUTPUT_ARCH.dmg"
rm -f "$DMG"
hdiutil create -quiet -fs HFS+ -volname DrillForge -srcfolder "$APP" "$DMG"
shasum -a 256 "$DMG" > "$DMG.sha256"
echo "$DMG"
