# Packaging

Adapted from Schist's `packaging/`, cut down to what Onionskin can honestly
produce today: one binary, `onionskin`, unsigned, without icons.
`.github/workflows/release.yml` runs all of this on a `vX.Y.Z` tag, or on a
manual dispatch, for Linux (x86_64 and aarch64), macOS and Windows.

Every script packages the shell-enabled release build, expects it to exist
already, and fails loudly if it does not:

```
cargo build --release -p onionskin-app --features shell
./packaging/linux/package.sh   # dist/onionskin-<version>-linux-<arch>.tar.gz
./packaging/macos/bundle.sh    # dist/Onionskin.app, dist/Onionskin-<version>-macos-<arch>.zip
makensis -DVERSION=<version> packaging/windows/installer.nsi
```

`version.sh` resolves the version once for all of them: `$VERSION` when CI
set it from the tag, the workspace manifest otherwise. On a tag build CI also
sets `VERSION_REQUIRED=1`, which turns a tag that is not semver-shaped into a
failed job rather than a release stamped with the manifest version.

## What is stubbed, and what it is waiting for

| Stubbed | Waiting for |
|---|---|
| No icons anywhere. The `.desktop` entry names `Icon=onionskin` with no file to match, the macOS bundle has no `CFBundleIconFile`, the NSIS script sets no `Icon`. | The in-house icon set. PLAN.md redraws every icon rather than reusing Adobe artwork, generated the way Schist generates its logo. Needs `onionskin.icns`, `onionskin.ico` and a 256x256 PNG. |
| No AppImage. Linux ships a tarball and a loose binary. | The icon above: `appimagetool` resolves the icon through the desktop entry's `Icon=` key and will not build without it. |
| macOS ships Apple Silicon only. The release matrix has one `macos-latest` runner, which is arm64, so an Intel Mac gets no artifact. | A `macos-13` (x86_64) matrix entry, or `lipo` joining the two builds into one universal binary. The zip is already versioned and arch-suffixed, so a second architecture publishes into the same release without colliding. |
| macOS builds are unsigned and un-notarized, so Gatekeeper blocks them on a machine that has not seen them. | A Developer ID Application certificate. Then: a keychain-import step in the release workflow, and `codesign --options runtime --timestamp --entitlements` plus `notarytool submit --wait` and `stapler staple` in `bundle.sh`. Secrets needed: `MACOS_CERT_P12_BASE64`, `MACOS_CERT_P12_PASSWORD`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`. Schist's `release.yml` and `macos/bundle.sh` are the reference. |
| Windows builds are unsigned. | An Authenticode certificate and a `signtool` step. |
| `CFBundleIdentifier` is the placeholder `org.onionskin.Onionskin`. | A settled domain. It must be final before the first signed release: Launch Services and the signing identity both key off it. |
| No MCP server binary in any artifact. | `crates/mcp` becoming a binary in M4. Schist ships its MCP server beside the app, loose rather than inside the bundle, because a client spawns it by path. |
| No hosted Linux/Windows first-release smoke validation yet. | B6 final acceptance on those hosts. The release workflow now builds the shell artifact and installs Linux GPUI build dependencies, but this macOS session cannot prove the Linux tarball or Windows installer launches the real viewer. |

## Rules that carry over from PLAN.md

- PDF is associated as an alternate handler on every platform, never the
  default. Onionskin joins the "Open with" menu; it does not take files off
  Acrobat.
- The Linux script strips DWARF from the shipped binary. The release profile
  sets `debug = 1` for readable local backtraces, and ELF carries that inside
  the executable.
