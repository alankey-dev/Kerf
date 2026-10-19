#!/bin/bash
# Writes the release notes for a version to stdout: its CHANGELOG.md section when there is one, then which
# artefacts are unsigned and how to open them. When CHANGELOG.md has no section for the version it prints
# only the footer and exits 3, so the release workflow can have GitHub add the merged pull requests to the
# notes instead: a missing changelog must not block a release. The second argument is a file listing the
# unsigned artefacts, one per line.
#
#   tools/release/notes.sh <version> <unsigned.txt>
set -euo pipefail

version=${1:?usage: notes.sh <version> <unsigned.txt>}
unsigned=${2:?usage: notes.sh <version> <unsigned.txt>}
root=$(cd "$(dirname "$0")/../.." && pwd)

section=$(awk -v v="$version" '
  index($0, "## [" v "]") == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on && /^\[[^]]+\]: / { exit }
  on { print }
' "$root/CHANGELOG.md")
status=0
if [[ -z ${section//[[:space:]]/} ]]; then
  status=3
else
  echo "$section"
fi

if [[ -s $unsigned ]]; then
  echo
  echo "### Unsigned artefacts"
  echo
  echo "These were not signed, so the system warns before opening them the first time:"
  echo
  sort -u "$unsigned" | while read -r name; do echo "- \`$name\`"; done
  echo
  echo "- **macOS:** right-click Kerf in Applications, choose Open, then Open again."
  echo "- **Windows:** in the SmartScreen prompt, choose More info, then Run anyway."
fi
echo
echo "Check a download against \`SHA256SUMS\` with \`sha256sum --check --ignore-missing SHA256SUMS\` (Linux) or \`shasum -a 256 --check --ignore-missing SHA256SUMS\` (macOS)."
exit "$status"
