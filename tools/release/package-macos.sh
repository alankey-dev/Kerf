#!/bin/bash
# Builds Kerf.app and Kerf-<version>-macos-universal.dmg in target/dist/.
#
#   tools/release/package-macos.sh <version>             universal app and .dmg (the release job)
#   tools/release/package-macos.sh <version> --app-only  this Mac's architecture, Kerf.app only (bundle.sh)
#
# With MACOS_CERT_P12 (base64), MACOS_CERT_PASSWORD, APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD set,
# the app is signed with the Developer ID certificate and the hardened runtime, and the .dmg is notarised
# and stapled. Without them the app is signed ad hoc, and target/dist/unsigned.txt names the .dmg.
set -euo pipefail

version=${1:?usage: package-macos.sh <version> [--app-only]}
app_only=${2:-}
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
dist=$root/target/dist
mkdir -p "$dist"

if [[ $app_only == --app-only ]]; then
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

app=$dist/Kerf.app
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

signed=
if [[ -n ${MACOS_CERT_P12:-} && -n ${APPLE_ID:-} ]]; then
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
  codesign --force --options runtime --timestamp --sign "$identity" "$app"
  signed=1
else
  # Ad hoc, as tools/macos/bundle.sh always did: Apple silicon will not run an unsigned binary at all.
  codesign --force --deep --sign - "$app"
fi

if [[ $app_only == --app-only ]]; then
  echo "$app"
  exit 0
fi

dmg=$dist/Kerf-$version-macos-universal.dmg
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname "Kerf $version" -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"

if [[ -n $signed ]]; then
  codesign --timestamp --sign "$identity" "$dmg"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$dmg"
else
  basename "$dmg" >> "$dist/unsigned.txt"
fi
echo "$dmg"
