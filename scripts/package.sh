#!/usr/bin/env bash
# Build the release packages (tarball, .deb, .rpm, AppImage) and the AUR files
# from an already built `target/release/graphene`.
#
# The version and the release tag are read from `graphene --version`: a dev build
# (`<version>-dev.<UTC timestamp>`) or a stable one (`<version>`).
#
# Usage: scripts/package.sh
# Output: dist/ (or $DIST_DIR)
#
# Requires: cargo-deb, cargo-generate-rpm, appimagetool (path in $APPIMAGETOOL,
# defaults to `appimagetool` on PATH).
# $GRAPHENE_BINARY overrides the binary read for the version, the tarball and the
# AppImage (default: target/release/graphene; the .deb and .rpm assets always come
# from Cargo.toml). Meant for tests.
set -euo pipefail

if [[ $# -ne 0 ]]; then
  echo "usage: $0" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

binary=${GRAPHENE_BINARY:-target/release/graphene}
dist=${DIST_DIR:-dist}
target=x86_64-unknown-linux-gnu

if [[ ! -x $binary ]]; then
  echo "missing $binary: run 'cargo build --release --locked' first" >&2
  exit 1
fi

version_output=$("$binary" --version)
if [[ ! $version_output =~ ^graphene\ (([0-9]+\.[0-9]+\.[0-9]+)(-dev\.([0-9]{14}))?)$ ]]; then
  echo "unexpected '$binary --version' output: $version_output" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}
base_version=${BASH_REMATCH[2]}
build=${BASH_REMATCH[4]}
tag="v${version}"
if [[ -n $build ]]; then
  deb_version="${base_version}~dev.${build}"
  rpm_release="0.dev.${build}"
  aur_pkgver="${base_version}.r${build}"
else
  deb_version=$base_version
  rpm_release=1
  aur_pkgver=$base_version
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
  --output "$dist/graphene-term_${version}_amd64.deb"

# RPM
cargo generate-rpm --set-metadata "release = \"${rpm_release}\"" \
  --output "$dist/graphene-term-${base_version}-${rpm_release}.x86_64.rpm"

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
