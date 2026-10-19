#!/bin/bash
# Fails unless the tag is `v` followed by Cargo.toml's version, so a release cannot ship under a version
# the binary does not report. Prints the version on success.
#
#   tools/release/check-version.sh <tag>     for example v0.2.0
set -euo pipefail

tag=${1:?usage: check-version.sh <tag>}
root=$(cd "$(dirname "$0")/../.." && pwd)
version=$(awk -F'"' '/^version = "/ { print $2; exit }' "$root/Cargo.toml")
if [[ $tag != "v$version" ]]; then
  echo "The tag $tag does not match Cargo.toml's version $version. Tag v$version, or run tools/release/bump.sh first." >&2
  exit 1
fi
echo "$version"
