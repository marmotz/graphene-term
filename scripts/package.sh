#!/usr/bin/env bash
# Build the release packages (tarball, .deb, .rpm, AppImage) and the AUR files
# from an already built `target/release/graphene`.
#
# Usage: scripts/package.sh <build-number>
# Output: dist/
#
# Requires: cargo-deb, cargo-generate-rpm, appimagetool (path in $APPIMAGETOOL,
# defaults to `appimagetool` on PATH).
set -euo pipefail

if [[ $# -ne 1 || ! $1 =~ ^[0-9]+$ ]]; then
  echo "usage: $0 <build-number>" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

build=$1
version=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "graphene") | .version')
target=x86_64-unknown-linux-gnu
tag="v${version}-dev.${build}"
deb_version="${version}~dev.${build}"
rpm_release="0.dev.${build}"
aur_pkgver="${version}.r${build}"
binary=target/release/graphene
dist=dist

if [[ ! -x $binary ]]; then
  echo "missing $binary: run 'cargo build --release --locked' first" >&2
  exit 1
fi

rm -rf "$dist"
mkdir -p "$dist"

# Tarball
dirname="graphene-${tag}-${target}"
archive="${dirname}.tar.gz"
mkdir -p "$dist/$dirname"
cp "$binary" packaging/graphene.desktop packaging/graphene.svg \
  README.md LICENSE-MIT LICENSE-APACHE "$dist/$dirname/"
tar -C "$dist" -czf "$dist/$archive" "$dirname"
rm -rf "${dist:?}/$dirname"

# Debian (no "~" in the file name: GitHub turns it into "." in release assets)
cargo deb --no-build --no-strip --deb-version "$deb_version" \
  --output "$dist/graphene-term_${version}-dev.${build}_amd64.deb"

# RPM
cargo generate-rpm --set-metadata "release = \"${rpm_release}\"" \
  --output "$dist/graphene-term-${version}-${rpm_release}.x86_64.rpm"

# AppImage
appdir="$dist/Graphene.AppDir"
install -Dm755 "$binary" "$appdir/usr/bin/graphene"
install -Dm644 packaging/graphene.desktop "$appdir/graphene.desktop"
install -Dm644 packaging/graphene.desktop "$appdir/usr/share/applications/graphene.desktop"
# A top-level PNG named after Icon= is required: appimagetool builds .DirIcon from it.
install -Dm644 packaging/graphene.png "$appdir/graphene.png"
install -Dm644 packaging/graphene.png "$appdir/usr/share/icons/hicolor/256x256/apps/graphene.png"
install -Dm644 packaging/graphene.svg "$appdir/usr/share/icons/hicolor/scalable/apps/graphene.svg"
ln -s usr/bin/graphene "$appdir/AppRun"
ARCH=x86_64 VERSION="$tag" "${APPIMAGETOOL:-appimagetool}" --no-appstream \
  "$appdir" "$dist/graphene-${tag}-x86_64.AppImage"
rm -rf "$appdir"

# Checksums
(cd "$dist" && sha256sum -- *.tar.gz *.deb *.rpm *.AppImage > SHA256SUMS)

# AUR (rendered against the tarball published with this release)
sha256=$(sha256sum "$dist/$archive" | cut -d' ' -f1)
mkdir -p "$dist/aur"
for template in PKGBUILD SRCINFO; do
  out=$template
  [[ $template == SRCINFO ]] && out=.SRCINFO
  sed -e "s|@PKGVER@|${aur_pkgver}|g" \
    -e "s|@TAG@|${tag}|g" \
    -e "s|@ARCHIVE@|${archive}|g" \
    -e "s|@DIRNAME@|${dirname}|g" \
    -e "s|@SHA256@|${sha256}|g" \
    "packaging/aur/${template}.in" > "$dist/aur/$out"
done

echo "tag=${tag}"
