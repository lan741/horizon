#!/usr/bin/env bash
# Build a Horizon.app bundle from the Rust executable and pack it into an .ipa.
#
# The port does not need an Xcode project: the executable is the app, and everything an iOS app
# bundle is made of (Info.plist, icons, PkgInfo, the code signature) is assembled here. Run it
# after `cargo build --release --target aarch64-apple-ios -p horizon-ios` on macOS.
#
#   BIN=target/aarch64-apple-ios/release/horizon-ios \
#   VERSION=0.1.0 BUILD=42 packaging/ios/make-ipa.sh
#
# Environment:
#   BIN             the built executable            (default target/aarch64-apple-ios/release/horizon-ios)
#   OUT             where the .app and .ipa go      (default dist/ios)
#   VERSION         CFBundleShortVersionString      (default 0.1.0)
#   BUILD           CFBundleVersion                 (default 1)
#   BUNDLE_ID       CFBundleIdentifier              (default dev.horizon.ios)
#   SIGN_IDENTITY   codesign identity, "-" is ad-hoc (default -)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

BIN="${BIN:-target/aarch64-apple-ios/release/horizon-ios}"
OUT="${OUT:-dist/ios}"
VERSION="${VERSION:-0.1.0}"
BUILD="${BUILD:-1}"
BUNDLE_ID="${BUNDLE_ID:-dev.horizon.ios}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
APP_NAME="Horizon"

cd "$root"

if [ ! -f "$BIN" ]; then
    echo "make-ipa: no executable at $BIN" >&2
    echo "          build it first: cargo build --release --target aarch64-apple-ios -p horizon-ios" >&2
    exit 1
fi

echo "==> assembling $APP_NAME.app ($VERSION build $BUILD, $BUNDLE_ID)"
rm -rf "$OUT"
mkdir -p "$OUT/Payload/$APP_NAME.app"

cp "$BIN" "$OUT/Payload/$APP_NAME.app/$APP_NAME"
chmod 755 "$OUT/Payload/$APP_NAME.app/$APP_NAME"

# Info.plist: the placeholders keep the checked-in file versionless.
SDK_VERSION="$(xcrun --sdk iphoneos --show-sdk-version 2>/dev/null || echo 18.0)"
SDK_BUILD="$(xcrun --sdk iphoneos --show-sdk-build-version 2>/dev/null || echo 22A000)"
OS_BUILD="$(sw_vers -buildVersion 2>/dev/null || echo 24A000)"
sed -e "s/@VERSION@/$VERSION/g" \
    -e "s/@BUILD@/$BUILD/g" \
    -e "s/@BUNDLE_ID@/$BUNDLE_ID/g" \
    -e "s/@SDK@/$SDK_VERSION/g" \
    -e "s/@SDKBUILD@/$SDK_BUILD/g" \
    -e "s/@OSBUILD@/$OS_BUILD/g" \
    "$here/Info.plist" > "$OUT/Payload/$APP_NAME.app/Info.plist"
plutil -lint "$OUT/Payload/$APP_NAME.app/Info.plist" >/dev/null

# PkgInfo: four bytes of type, four of creator, as every bundle carries.
printf 'APPL????' > "$OUT/Payload/$APP_NAME.app/PkgInfo"

if [ -d "$here/AppIcon" ]; then
    cp "$here/AppIcon"/*.png "$OUT/Payload/$APP_NAME.app/"
fi

# Thin the binary of local symbols only; names in backtraces survive.
if command -v strip >/dev/null 2>&1; then
    strip -x "$OUT/Payload/$APP_NAME.app/$APP_NAME" || true
fi

echo "==> signing with identity '$SIGN_IDENTITY'"
if [ "$SIGN_IDENTITY" = "-" ]; then
    codesign --force --sign - --timestamp=none "$OUT/Payload/$APP_NAME.app"
else
    codesign --force --sign "$SIGN_IDENTITY" --timestamp=none --options runtime \
        "$OUT/Payload/$APP_NAME.app"
fi
codesign --verify --deep --strict --verbose=2 "$OUT/Payload/$APP_NAME.app" 2>&1 | tail -3 || true

ipa="$OUT/$APP_NAME-$VERSION.ipa"
echo "==> packing $ipa"
(cd "$OUT" && zip -qry "$APP_NAME-$VERSION.ipa" Payload)

size=$(du -h "$ipa" | cut -f1)
echo "==> $ipa ($size)"
if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$ipa"
fi