#!/usr/bin/env bash
# Build an Arch Linux package (graphene-term-bin) from a GitHub release tarball,
# using the AUR PKGBUILD template.
#
# Usage: scripts/build-arch-package.sh [version]
#   version: a release tag (`v0.1.0`, `v0.1.0-dev.20261113205256`) or the same
#            without the leading `v`. Default: the most recent release, pre-releases
#            included.
# Output: dist/arch/graphene-term-bin-<pkgver>-1-x86_64.pkg.tar.zst (or $DIST_DIR).
# Prints the package path on stdout, and the `pacman -U` command to install it on stderr.
#
# Requires: curl, jq, makepkg.
# Overrides, meant for tests: $GRAPHENE_RELEASES_API (releases API endpoint),
# $GRAPHENE_DOWNLOAD_URL (base URL of the release assets), $MAKEPKG.
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [version]" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

repo=marmotz/graphene-term
api=${GRAPHENE_RELEASES_API:-https://api.github.com/repos/${repo}/releases}
download=${GRAPHENE_DOWNLOAD_URL:-https://github.com/${repo}/releases/download}
makepkg=${MAKEPKG:-makepkg}
dist=${DIST_DIR:-dist/arch}
target=x86_64-unknown-linux-gnu

if [[ $# -eq 1 ]]; then
  tag="v${1#v}"
else
  tag=$(curl -sSfL "${api}?per_page=1" | jq -r '.[0].tag_name // empty')
  if [[ -z $tag ]]; then
    echo "no release found at $api" >&2
    exit 1
  fi
fi

if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)(-dev\.([0-9]{14}))?$ ]]; then
  echo "unexpected version: $tag" >&2
  exit 1
fi
base_version=${BASH_REMATCH[1]}
build=${BASH_REMATCH[3]}
if [[ -n $build ]]; then
  pkgver="${base_version}.r${build}"
else
  pkgver=$base_version
fi

dirname="graphene-${tag}-${target}"
archive="${dirname}.tar.gz"

sums=$(curl -sSfL "${download}/${tag}/SHA256SUMS") || {
  echo "cannot download SHA256SUMS of release $tag" >&2
  exit 1
}
sha256=$(awk -v f="$archive" '$2 == f { print $1 }' <<< "$sums")
if [[ -z $sha256 ]]; then
  echo "$archive is not listed in the SHA256SUMS of release $tag" >&2
  exit 1
fi

mkdir -p "$dist"
dist=$(cd "$dist" && pwd)
builddir="$dist/build"
rm -rf "$builddir"
mkdir -p "$builddir"
trap 'rm -rf "$builddir"' EXIT

sed -e "s|@PKGVER@|${pkgver}|g" \
  -e "s|@TAG@|${tag}|g" \
  -e "s|@ARCHIVE@|${archive}|g" \
  -e "s|@DIRNAME@|${dirname}|g" \
  -e "s|@SHA256@|${sha256}|g" \
  packaging/aur/PKGBUILD.in > "$builddir/PKGBUILD"

echo "building graphene-term-bin ${pkgver} from release ${tag}" >&2
(cd "$builddir" && PKGDEST="$dist" "$makepkg" --force --cleanbuild >&2)
package=$(cd "$builddir" && PKGDEST="$dist" "$makepkg" --packagelist)

echo "$package"
# On stderr: stdout stays the package path, for callers.
{
  echo
  echo "install it with:"
  echo "  sudo pacman -U ${package//$'\n'/ }"
} >&2
