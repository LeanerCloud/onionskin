#!/usr/bin/env bash
# Build Onionskin.app from the release build and zip it into dist/.
#
# Build first: cargo build --release -p onionskin-app
# Unsigned and un-notarized; see packaging/README.md for what signing needs.
# Only the zip is safe to hand to CI artifact upload, which preserves neither
# a bundle's permissions nor its symlinks.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
bin="$root/target/release/onionskin"

if [ ! -x "$bin" ]; then
    echo "no binary at $bin; run: cargo build --release -p onionskin-app" >&2
    exit 1
fi

. "$root/packaging/version.sh"
version="$(onionskin_version "$root")"
arch="$(uname -m)"
app="$root/dist/Onionskin.app"
# Versioned and arch-suffixed like the Linux and Windows artifacts, so an
# Intel or universal build can publish into the same release later.
zip="$root/dist/Onionskin-$version-macos-$arch.zip"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$root/dist"
cp "$root/packaging/macos/Info.plist" "$app/Contents/Info.plist"
cp "$bin" "$app/Contents/MacOS/onionskin"

# Stamped rather than hardcoded, so the bundle's version cannot drift from
# the workspace manifest.
/usr/libexec/PlistBuddy \
    -c "Set :CFBundleVersion $version" \
    -c "Set :CFBundleShortVersionString $version" \
    "$app/Contents/Info.plist"

rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

echo "built $app"
echo "built $zip"
