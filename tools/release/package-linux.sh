#!/bin/bash
# Builds the Linux release artefacts for this machine's architecture into target/dist/:
#   Kerf-<version>-linux-<arch>.deb, .AppImage and .tar.gz
#
#   tools/release/package-linux.sh <version>
#
# Needs the packages in tools/release/linux-deps.txt, cargo-deb (`cargo install cargo-deb`), and network
# access the first time, to fetch linuxdeploy. Build on the oldest distribution you support: the glibc it
# links against is the oldest one the binary runs on.
set -euo pipefail

version=${1:?usage: package-linux.sh <version>}
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
arch=$(uname -m)
dist=$root/target/dist
work=$root/target/linux-package
mkdir -p "$dist"
rm -rf "$work"
mkdir -p "$work"

cargo build --release --locked

# The paths here match the assets in Cargo.toml's [package.metadata.deb].
cat > "$work/kerf.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kerf
GenericName=Cutting machine editor
Comment=Design and cut on Cricut, HPGL and G-code cutters
Exec=kerf
Icon=kerf
Terminal=false
Categories=Graphics;VectorGraphics;
Keywords=cricut;cutter;vinyl;svg;
DESKTOP
for size in 16 24 32 48 64 128 256 512; do
  mkdir -p "$work/icons/${size}x${size}/apps"
  cp "tools/release/icons/kerf-$size.png" "$work/icons/${size}x${size}/apps/kerf.png"
done

cargo deb --no-build --deb-version "$version" --output "$dist/Kerf-$version-linux-$arch.deb"

# linuxdeploy copies the binary's shared libraries into the AppImage, except the ones every desktop has.
# Pinned by release and checksum.
linuxdeploy=$root/target/linuxdeploy-$arch.AppImage
case $arch in
  x86_64) sum=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d ;;
  aarch64) sum=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff ;;
  *) echo "No linuxdeploy checksum for $arch" >&2; exit 1 ;;
esac
if [[ ! -f $linuxdeploy ]]; then
  curl -fsSL -o "$linuxdeploy" "https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-$arch.AppImage"
fi
echo "$sum  $linuxdeploy" | sha256sum --check --quiet
chmod +x "$linuxdeploy"
icons=()
for size in 16 32 48 64 128 256 512; do
  icons+=(--icon-file "$work/icons/${size}x${size}/apps/kerf.png")
done
# CI runners have no FUSE, so the AppImage tools unpack themselves instead of mounting.
export APPIMAGE_EXTRACT_AND_RUN=1
(cd "$work" && ARCH=$arch LDAI_OUTPUT="$dist/Kerf-$version-linux-$arch.AppImage" \
  "$linuxdeploy" --appdir AppDir --executable "$root/target/release/kerf" --desktop-file kerf.desktop "${icons[@]}" --output appimage)

tarball=Kerf-$version-linux-$arch
rm -rf "${work:?}/$tarball"
mkdir -p "$work/$tarball"
cp target/release/kerf "$work/kerf.desktop" "$work/$tarball/"
cp -R "$work/icons" "$work/$tarball/icons"
tar -C "$work" -czf "$dist/$tarball.tar.gz" "$tarball"

ls -la "$dist"
