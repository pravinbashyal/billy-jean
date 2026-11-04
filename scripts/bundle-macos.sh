#!/usr/bin/env bash
# Builds a release binary for one macOS target and packages it as
# "Billy Jean.app" inside dist/Billy-Jean-<version>-<arch>.dmg.
#
# Usage: scripts/bundle-macos.sh <target-triple> <version>
#   e.g. scripts/bundle-macos.sh aarch64-apple-darwin 1.0.0
#
# The ad-hoc signature is applied as the last change to the bundle. Nothing
# modifies the .app after codesign runs, otherwise the seal would be broken.
set -euo pipefail

usage() {
  echo "usage: $0 <target-triple> <version>" >&2
  echo "  target-triple: aarch64-apple-darwin | x86_64-apple-darwin" >&2
  echo "  version:       X.Y.Z" >&2
  exit 2
}

[[ $# -eq 2 ]] || usage
triple="$1"
version="$2"

case "$triple" in
  aarch64-apple-darwin) arch="arm64" ;;
  x86_64-apple-darwin) arch="x86_64" ;;
  *) echo "error: unsupported target triple '$triple'" >&2; usage ;;
esac

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version '$version' is not X.Y.Z" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

app_name="Billy Jean.app"
binary_name="billing"
dmg_name="Billy-Jean-${version}-${arch}.dmg"
dist_dir="$root/dist"
stage_dir="$dist_dir/stage-${arch}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

# Stock macOS 11 is the floor (LSMinimumSystemVersion in packaging/Info.plist).
export MACOSX_DEPLOYMENT_TARGET=11.0

echo "==> cargo build --release --locked --target $triple"
cargo build --release --locked --target "$triple"

binary="$target_dir/$triple/release/$binary_name"
if [[ ! -x "$binary" ]]; then
  echo "error: release binary not found at $binary" >&2
  exit 1
fi

echo "==> assembling $app_name in $stage_dir"
rm -rf "${stage_dir:?}" "${dist_dir:?}/${dmg_name:?}"
mkdir -p "$stage_dir"
app="$stage_dir/$app_name"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"

cp "$root/packaging/Info.plist" "$contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$version" "$contents/Info.plist"
plutil -replace CFBundleVersion -string "$version" "$contents/Info.plist"
plutil -lint "$contents/Info.plist" >/dev/null

cp "$binary" "$contents/MacOS/$binary_name"
chmod 755 "$contents/MacOS/$binary_name"

# SIL Open Font License for the embedded Noto Sans (see assets/fonts/README.md).
cp "$root/assets/fonts/OFL.txt" "$contents/Resources/OFL.txt"

echo "==> ad-hoc signing $app_name (last change to the bundle)"
codesign --force --deep --sign - "$app"

echo "==> creating $dmg_name"
ln -s /Applications "$stage_dir/Applications"
mkdir -p "$dist_dir"
hdiutil create \
  -quiet \
  -format UDZO \
  -volname "Billy Jean $version" \
  -srcfolder "$stage_dir" \
  "$dist_dir/$dmg_name"

rm -rf "$stage_dir"
echo "==> wrote $dist_dir/$dmg_name"
