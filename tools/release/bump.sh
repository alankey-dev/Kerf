#!/bin/bash
# Sets the version in Cargo.toml and Cargo.lock and prints the commands that tag the release. It changes
# files only: it does not commit, tag or push.
#
#   tools/release/bump.sh <version>     for example 0.3.0
set -euo pipefail

version=${1:?usage: bump.sh <version>}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "Not a semantic version: $version" >&2; exit 1; }
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

# Only the first `version =` line, which is [package]'s.
awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' Cargo.toml > Cargo.toml.new
mv Cargo.toml.new Cargo.toml
cargo update --workspace --quiet

echo "Set the version to $version. Commit, then tag and push:"
echo "  git tag -a v$version -m \"Kerf $version\""
echo "  git push origin v$version"
