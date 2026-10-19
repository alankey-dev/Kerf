# 41 Build and release on Windows, macOS and Linux

**Status: Implemented.** v0.0.1 was released from the tag-triggered `Release` workflow on 2026-10-19; the installers are in the GitHub Release.

**Wave 9. Depends on everything merged. Size L.** Requested directly by the user: "building and releasing on multiple platforms (windows, Mac, Linux)".

## Goal

Kerf builds, tests and ships on all three desktop platforms from one tag. Pushing `v0.2.0` produces a GitHub Release with an installer for each platform, checksums and notes, and no step on a developer's laptop.

Today CI runs only on `macos-latest`, and the only package is `tools/macos/bundle.sh`, which installs an ad hoc signed `Kerf.app` locally.

## CI on three platforms

`.github/workflows/ci.yml` becomes a matrix over `macos-latest`, `windows-latest` and `ubuntu-latest`. Every leg runs format, clippy, test and advisories, as macOS does now.

- **Rendering.** The kittests render through wgpu. macOS uses Metal. Windows uses the WARP software adapter through DX12. Linux installs `mesa-vulkan-drivers` (lavapipe) and runs under `xvfb-run`. Put each platform's setup in its own step with a comment that says why it is there.
- **Snapshots.** Software renderers anti-alias differently from Metal. If a snapshot test fails only on one platform, look at the diff image first. Raise that test's `SnapshotOptions::threshold` only when the diff is edge noise, and name the platform and the measured difference in a comment. Never skip, ignore or `cfg`-out a test to get green.
- **Linux system packages.** Install only what the build proves it needs (start with `libxkbcommon-dev`, `libwayland-dev`, `libgtk-3-dev` for the file dialog, and `libudev-dev` only if `serialport` asks for it). List them once in `tools/release/linux-deps.txt` and read that file from CI and from the release job.
- **Cache.** Keep `Swatinem/rust-cache@v2`, keyed per OS.

## Platform gaps to close in the app

These are the places the code assumes macOS. Fix each one where it lives, and add a test where the behaviour is pure.

1. **Shortcut labels.** egui's `format_shortcut` already prints `Ctrl` off macOS. A few strings hard-code `Cmd` (for example the Measure rail tooltip, "Cmd+C copies"). Route every one through `format_shortcut`. Add a test that fails if a string literal under `src/ui/` contains `Cmd+` or `⌘`.
2. **System fonts.** `system_fonts.rs` must scan `%WINDIR%\Fonts` and `%LOCALAPPDATA%\Microsoft\Windows\Fonts` on Windows, and the fontconfig directories (`/usr/share/fonts`, `/usr/local/share/fonts`, `~/.local/share/fonts`) on Linux. Unit-test the directory list per platform with `cfg`.
3. **Serial ports.** Open cutters list `COM3` on Windows and `/dev/ttyUSB0` or `/dev/ttyACM0` on Linux. Check that `device_setup.rs` shows these names, and that a device profile saved on one platform with a port that does not exist on another loads with "Port not found" instead of an error.
4. **Projects directory.** `directories` already picks the right base folder. Add one test per platform that prints the resolved path, so a reviewer can see it in the CI log.
5. **Console window.** Release builds on Windows set `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`. `--version`, `--bench` and `--bench-generate` call `AttachConsole(ATTACH_PARENT_PROCESS)` first, so their output still reaches a terminal.
6. **`kerf --version`.** Add it. It prints `kerf 0.2.0 (abc1234)` using `ODS_GIT_COMMIT` from `build.rs`, and exits before eframe starts, so it runs on a runner with no display. The release job uses it as a smoke test.

## Icons

Move `tools/macos/icon.svg` to `tools/release/icon.svg`, the single source. `tools/release/icons.rs`, a small example binary (`cargo run --example icons`), renders it with `resvg` (already in the tree through `usvg`) to every size each platform needs: `.icns` inputs for macOS, a multi-size `kerf.ico` for Windows and PNGs from 16 to 512 px for Linux. Commit the generated files so a release does not depend on `qlmanage` or `iconutil`. On Windows, `build.rs` embeds `kerf.ico` and the version info with the `winresource` build-dependency, under `cfg(windows)` only.

## Release workflow

`.github/workflows/release.yml` runs on a pushed tag `v*` and on `workflow_dispatch` (dry run, no publish).

1. **Check.** Fail at once if the tag does not equal `v` + `Cargo.toml`'s `version`.
2. **Build matrix.**

   | Runner | Target | Artefacts |
   |---|---|---|
   | `macos-latest` | `aarch64-apple-darwin` and `x86_64-apple-darwin`, joined with `lipo` | `Kerf-<v>-macos-universal.dmg` |
   | `windows-latest` | `x86_64-pc-windows-msvc` | `Kerf-<v>-windows-x64.msi`, `Kerf-<v>-windows-x64.zip` (portable) |
   | `windows-latest` | `aarch64-pc-windows-msvc` | `Kerf-<v>-windows-arm64.msi`, `.zip` |
   | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `Kerf-<v>-linux-x86_64.AppImage`, `.deb`, `.tar.gz` |
   | `ubuntu-22.04-arm` | `aarch64-unknown-linux-gnu` | the same three for `aarch64` |

   Build Linux on 22.04 so the glibc floor stays old enough for current LTS distributions.
3. **Package.** Each leg calls one script under `tools/release/`, so a developer can run the same packaging locally:
   - `package-macos.sh <version>` builds `Kerf.app` (the plist from `bundle.sh`, moved here) and a `.dmg` with `hdiutil`. `tools/macos/bundle.sh` stays for local installs and calls this script for the `.app`.
   - `package-windows.ps1 <version>` writes the `.zip` and builds the `.msi` with `cargo-wix` from `wix/main.wxs` (Start menu shortcut, per-user install, upgrade code fixed in the file so upgrades replace older versions).
   - `package-linux.sh <version>` writes `kerf.desktop` and the icons, builds the `.deb` with `cargo-deb` (metadata in `Cargo.toml` under `[package.metadata.deb]`), the AppImage with `linuxdeploy`, and the `.tar.gz`.
4. **Signing.** Sign only when the secrets exist, and produce unsigned artefacts otherwise, so forks and dry runs still work.
   - macOS: Developer ID signing with the hardened runtime, then `xcrun notarytool submit --wait` and `xcrun stapler staple`. Secrets: `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`. Without them, sign ad hoc as `bundle.sh` does now.
   - Windows: Authenticode with `signtool` on the `.exe` and the `.msi`. Secrets: `WINDOWS_CERT_PFX`, `WINDOWS_CERT_PASSWORD`.
   - The release notes say which artefacts are unsigned, and how to open them (right-click Open on macOS, More info then Run anyway on Windows).
5. **Smoke test.** Each leg installs or unpacks its own artefact and runs `kerf --version`. The output must match the tag.
6. **Publish.** One final job downloads every artefact, writes `SHA256SUMS`, and creates a draft GitHub Release with the notes from the tag's section of `CHANGELOG.md`. A maintainer reviews the draft and publishes it by hand.

Pin every third-party action to a full commit SHA, with the version in a comment. Give the publish job `contents: write` and every other job `contents: read`.

## Versioning

`CHANGELOG.md` starts with a `## [0.2.0]` section in Keep a Changelog form. `tools/release/bump.sh <version>` sets `Cargo.toml`, refreshes `Cargo.lock`, adds an empty changelog section and prints the `git tag` command. It does not push.

## Files

- **Owns.** `.github/workflows/release.yml`, `tools/release/` (scripts, `icon.svg`, generated icons, `linux-deps.txt`, `wix/`), `examples/icons.rs`, `CHANGELOG.md`, the `--version` path in `src/main.rs`.
- **Shared edits.** `.github/workflows/ci.yml` (the matrix), `build.rs` (Windows resources), `Cargo.toml` (`winresource` build-dependency, `[package.metadata.deb]`, `[package.metadata.wix]`), `src/system_fonts.rs` (the directory list), the `Cmd` strings in `src/ui/`, `tools/macos/bundle.sh` (call the shared script), `README.md` (an Install section, and the icon path at the top).

## Acceptance

- [ ] CI is green on macOS, Windows and Linux on the same commit, with no skipped or ignored tests added. Each raised snapshot threshold has a comment naming the platform and the measured difference.
- [ ] No string literal under `src/ui/` contains `Cmd+` or `⌘`, and a test enforces it.
- [ ] System fonts are found on a Windows and a Linux runner. Paste the count each one logs.
- [ ] `kerf --version` prints the version and commit on all three platforms, and exits without opening a window.
- [ ] A `workflow_dispatch` dry run of `release.yml` produces all eleven artefacts and `SHA256SUMS`, and every smoke test passes. Link the run.
- [ ] A tag whose version does not match `Cargo.toml` fails in the first job.
- [ ] With no signing secrets set, the workflow still finishes, and the draft notes list the unsigned artefacts.
- [ ] The `.msi` installs, shows Kerf in the Start menu, and a second `.msi` of a higher version replaces it. The `.deb` installs on Ubuntu 22.04 and 24.04, and the AppImage starts on both.
- [ ] `README.md` has an Install section that links to the latest release and lists the artefact for each platform.
