#!/bin/zsh
# Builds Kerf.app for this Mac with tools/release/package-macos.sh and installs it to /Applications (or the
# folder given as $1).
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
root=${0:A:h:h:h}
dest=${1:-/Applications}
cd "$root"
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["packages"][0]["version"])')
app=$(tools/release/package-macos.sh "$version" --app-only | tail -1)
rm -rf "$dest/Kerf.app"
cp -R "$app" "$dest/"
echo "Installed $dest/Kerf.app ($version, $(git rev-parse --short HEAD))"
