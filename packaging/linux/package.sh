#!/usr/bin/env bash
# Package the Linux release build into dist/: a loose binary and a tarball.
#
# Build first: cargo build --release -p onionskin-app
# An AppImage lands once the icon set exists, see packaging/README.md.
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
stage="$root/target/package/onionskin-$version-linux-$arch"

rm -rf "$stage"
mkdir -p "$stage" "$root/dist"
cp "$bin" "$stage/onionskin"
# The release profile keeps debug = 1 for readable local backtraces, and ELF
# embeds that in the executable itself. Strip the DWARF so the artifact does
# not carry it; symbols stay.
strip --strip-debug "$stage/onionskin"
cp "$root/LICENSE" "$stage/"
cp "$root/packaging/linux/onionskin.desktop" "$stage/"

tar -czf "$root/dist/onionskin-$version-linux-$arch.tar.gz" \
    -C "$(dirname "$stage")" "$(basename "$stage")"
# Arch-suffixed: the x86_64 and aarch64 jobs publish into one release, where
# same-named assets would collide.
cp "$stage/onionskin" "$root/dist/onionskin-linux-$arch"

echo "built $root/dist/onionskin-$version-linux-$arch.tar.gz"
echo "built $root/dist/onionskin-linux-$arch"
