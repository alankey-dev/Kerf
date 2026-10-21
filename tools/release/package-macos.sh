#!/bin/bash
# Builds Kerf.app and Kerf-<version>-macos-universal.dmg in target/dist/, in two phases so that no signing
# secret is ever in the environment of `cargo build` (every build.rs and proc-macro in the graph can read it).
#
#   tools/release/package-macos.sh <version>             build phase: universal app, signed ad hoc, and .dmg
#   tools/release/package-macos.sh <version> --sign      sign phase: signs, notarises and staples the built app
#   tools/release/package-macos.sh <version> --app-only  this Mac's architecture, Kerf.app only (bundle.sh)
#
# The build phase ignores the signing variables and leaves the app at target/sign-input/Kerf.app.tar. The
# sign phase runs no cargo: with MACOS_CERT_P12 (base64), MACOS_CERT_PASSWORD, APPLE_ID, APPLE_TEAM_ID and
# APPLE_APP_PASSWORD set, it signs that app with the Developer ID certificate and the hardened runtime, then
# rebuilds the .dmg and notarises and staples it. Without them it keeps the ad hoc build and
# target/dist/unsigned.txt names the .dmg.
set -euo pipefail

usage="usage: package-macos.sh <version> [--app-only | --sign]"
version=${1:?$usage}
mode=${2:-build}
case $mode in
  build | --app-only | --sign) ;;
  *) echo "$usage" >&2; exit 2 ;;
esac
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
dist=$root/target/dist
stash=$root/target/sign-input
app=$dist/Kerf.app
dmg=$dist/Kerf-$version-macos-universal.dmg
mkdir -p "$dist"

make_dmg() {
  local stage
  stage=$(mktemp -d)
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  rm -f "$dmg"
  hdiutil create -volname "Kerf $version" -srcfolder "$stage" -ov -format UDZO "$dmg"
  rm -rf "$stage"
}

if [[ $mode == --sign ]]; then
  if [[ -z ${MACOS_CERT_P12:-} || -z ${APPLE_ID:-} || -z ${APPLE_TEAM_ID:-} || -z ${APPLE_APP_PASSWORD:-} ]]; then
    echo "No signing secrets: keeping the ad hoc build." >&2
    basename "$dmg" >> "$dist/unsigned.txt"
    exit 0
  fi
  rm -rf "$app"
  tar -C "$dist" -xf "$stash/Kerf.app.tar"
  keychain=$RUNNER_TEMP/kerf-signing.keychain-db
  password=$(uuidgen)
  security create-keychain -p "$password" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$password" "$keychain"
  echo "$MACOS_CERT_P12" | base64 --decode > "$RUNNER_TEMP/cert.p12"
  security import "$RUNNER_TEMP/cert.p12" -k "$keychain" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain"
  # Put the keychain search list back and drop the signing keychain and certificate on the way out, however
  # the script ends, so a build never leaves the machine's keychain list changed.
  searched=$(security list-keychains -d user | tr -d '"')
  # shellcheck disable=SC2086 # one argument per keychain in the saved list
  trap 'security list-keychains -d user -s $searched; security delete-keychain "$keychain" 2>/dev/null; rm -f "$RUNNER_TEMP/cert.p12"' EXIT
  # shellcheck disable=SC2046,SC2086 # one argument per keychain already in the search list
  security list-keychains -d user -s "$keychain" $searched
  identity=$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/Developer ID Application/ {print $2; exit}')
  [[ -n $identity ]] || { echo "No Developer ID Application identity in the certificate." >&2; exit 1; }
  codesign --force --options runtime --timestamp --sign "$identity" "$app"
  codesign --verify --strict --verbose=2 "$app"
  make_dmg
  codesign --timestamp --sign "$identity" "$dmg"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  echo "$dmg"
  exit 0
fi

# The build phase never signs with a certificate, so nothing here needs the secrets: drop them, so that a
# build script cannot read them even if the caller left them in the environment.
unset MACOS_CERT_P12 MACOS_CERT_PASSWORD APPLE_ID APPLE_TEAM_ID APPLE_APP_PASSWORD

if [[ $mode == --app-only ]]; then
  cargo build --release --locked
  binary=target/release/kerf
else
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$target"
  done
  binary=target/universal-apple-darwin/kerf
  mkdir -p "$(dirname "$binary")"
  lipo -create -output "$binary" target/aarch64-apple-darwin/release/kerf target/x86_64-apple-darwin/release/kerf
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/kerf"
cp tools/release/icons/Kerf.icns "$app/Contents/Resources/Kerf.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Kerf</string>
  <key>CFBundleDisplayName</key><string>Kerf</string>
  <key>CFBundleIdentifier</key><string>local.kerf.app</string>
  <key>CFBundleExecutable</key><string>kerf</string>
  <key>CFBundleIconFile</key><string>Kerf</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$(git rev-parse --short HEAD)</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict></plist>
PLIST

# Ad hoc, as tools/macos/bundle.sh always did: Apple silicon will not run an unsigned binary at all.
codesign --force --deep --sign - "$app"

if [[ $mode == --app-only ]]; then
  echo "$app"
  exit 0
fi

make_dmg
# The sign phase starts from this app. A tar keeps the permissions that an artefact upload would drop.
mkdir -p "$stash"
tar -C "$dist" -cf "$stash/Kerf.app.tar" Kerf.app
echo "$dmg"
