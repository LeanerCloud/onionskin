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

`--features shell` is what makes the binary the windowed viewer. Without it the
same `cargo build` succeeds and produces a headless binary that links no window
or GPU framework, which is an artifact nobody can open a PDF in.
`release_artifacts_build_the_windowed_viewer` in
`crates/app/tests/guarantees.rs` holds the workflow to it.

## What the build needs

The release workflow installs each of these, and the CI `shell` job installs
the same set. Building locally needs them too.

- `CARGO_NET_GIT_FETCH_WITH_CLI=true`. The shell pulls gpui, hayro and vello
  from pinned git revisions, and a gitconfig that rewrites `https` to `ssh`
  leaves libgit2 unable to authenticate them. Every workflow job exports it.
- macOS: `xcodebuild -downloadComponent MetalToolchain`. Without it gpui's
  Metal shaders do not compile under Xcode 26.
- Linux: `libfontconfig1-dev libvulkan-dev libwayland-dev libx11-xcb-dev
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
  libxkbcommon-dev libxkbcommon-x11-dev`.
- Windows: nothing beyond the toolchain for the build. `makensis` comes from
  NSIS, which is not on the runner image and the workflow installs through
  Chocolatey.

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
| No hosted Linux/Windows first-release smoke validation yet. | B6 final acceptance on those hosts. The macOS half is proved locally: `bundle.sh` over a `--features shell` release build produces an `Onionskin.app` whose binary links AppKit, CoreGraphics, QuartzCore and Metal, where the same script over a build without the feature packages a binary linking only `libSystem`. Nothing here can prove the Linux tarball or the Windows installer launches the real viewer, and no hosted release run has happened. |

## Rules that carry over from PLAN.md

- PDF is associated as an alternate handler on every platform, never the
  default. Onionskin joins the "Open with" menu; it does not take files off
  Acrobat.
- The Linux script strips DWARF from the shipped binary. The release profile
  sets `debug = 1` for readable local backtraces, and ELF carries that inside
  the executable.
