#!/usr/bin/env bash
# Build a macOS CLI archive and an unsigned drag-and-drop DMG containing Graphene.app.
#
# Usage: scripts/package-macos.sh <aarch64-apple-darwin|x86_64-apple-darwin>
# Requires: sips, iconutil, hdiutil.
# $GRAPHENE_BINARY and $DIST_DIR can be overridden for tests.
set -euo pipefail

if [[ $# -ne 1 || ! $1 =~ ^(aarch64|x86_64)-apple-darwin$ ]]; then
  echo "usage: $0 <aarch64-apple-darwin|x86_64-apple-darwin>" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

target=$1
binary=${GRAPHENE_BINARY:-target/$target/release/graphene}
dist=${DIST_DIR:-dist}

if [[ ! -x $binary ]]; then
  echo "missing $binary: run 'cargo build --release --target $target' first" >&2
  exit 1
fi

version_output=$("$binary" --version)
if [[ ! $version_output =~ ^graphene\ (([0-9]+\.[0-9]+\.[0-9]+)(-dev\.([0-9]{14}))?)$ ]]; then
  echo "unexpected '$binary --version' output: $version_output" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}
base_version=${BASH_REMATCH[2]}
tag="v${version}"

mkdir -p "$dist"
work=$(mktemp -d "${TMPDIR:-/tmp}/graphene-macos.XXXXXX")
trap 'rm -rf "$work"' EXIT

# CLI archive for users who prefer to run Graphene from their own terminal.
dirname="graphene-${tag}-${target}"
mkdir -p "$dist/$dirname"
install -m755 "$binary" "$dist/$dirname/graphene"
install -m644 README.md LICENSE-MIT LICENSE-APACHE "$dist/$dirname/"
COPYFILE_DISABLE=1 tar -C "$dist" -czf "$dist/${dirname}.tar.gz" "$dirname"
rm -rf "$dist/$dirname"

# Finder-installable app bundle, wrapped in a compressed disk image.
stage="$work/stage"
app="$stage/Graphene.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$binary" "$app/Contents/MacOS/graphene"

iconset="$work/graphene.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  for scale in 1 2; do
    pixels=$((size * scale))
    suffix=""
    [[ $scale == 2 ]] && suffix="@2x"
    sips -z "$pixels" "$pixels" packaging/graphene.png \
      --out "$iconset/icon_${size}x${size}${suffix}.png" >/dev/null
  done
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/graphene.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>graphene</string>
  <key>CFBundleIdentifier</key><string>dev.marmotz.graphene</string>
  <key>CFBundleName</key><string>Graphene</string>
  <key>CFBundleDisplayName</key><string>Graphene</string>
  <key>CFBundleIconFile</key><string>graphene</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${base_version}</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

ln -s /Applications "$stage/Applications"
cat > "$stage/Install Graphene.txt" <<'INSTALL'
Drag Graphene.app to Applications. The app is not signed or notarized; on first launch,
macOS may require Control-click > Open to confirm that you want to run it.
INSTALL

hdiutil create -volname "Graphene ${tag}" -srcfolder "$stage" -ov -format UDZO \
  "$dist/graphene-${tag}-${target}.dmg" >/dev/null
