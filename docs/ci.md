# Continuous integration

What runs on a pull request and on a release. The workflows are `.github/workflows/ci.yml` and `release.yml`. Everything runs on GitHub-hosted runners, which are free and unlimited for a public repository.

## What runs

Every pull request runs `ci.yml`, and nothing runs on a push: every change reaches `main` through a pull request, so a second run on the merge would repeat the checks.

| Job | What it does |
| --- | --- |
| `check` | Format, Clippy, the tests (under `xvfb-run` on Linux, where the kittests render through Mesa's lavapipe). Once each on Linux (`ubuntu-latest`), macOS (`macos-latest`) and Windows (`windows-latest`). The Linux leg also runs `cargo deny check advisories`; `deny.toml` lists all five release triples as `targets`, so that one run checks every platform's dependency graph. Check a single triple with `cargo deny -t <triple> check advisories`. |
| `timing` | The heavy-design and 200-shape frame-time gates (`docs/specs/38-benchmark.md`), on `macos-latest`, where the ceilings were set. |

The kittests render through wgpu: Metal on macOS, the WARP software adapter through DX12 on Windows, and Mesa's lavapipe Vulkan driver under `xvfb-run` on Linux. The Linux build needs the packages in `tools/release/linux-deps.txt`, which both `ci.yml` and the release job install from that file.

At home, `cargo test` and `cargo clippy --all-targets -- -D warnings` are what a pull request asks for.

## Releases

`release.yml` runs on a pushed `v*` tag, or by hand (`workflow_dispatch`) for a dry run that keeps the artefacts, `SHA256SUMS` and notes as a workflow artifact instead of drafting a release.

- `check` verifies the tag matches `Cargo.toml`'s version (`tools/release/check-version.sh`), so a release cannot ship under a version the binary does not report.
- `build` builds every platform's artefacts on GitHub-hosted runners: the universal macOS `.dmg` on `macos-latest`, the `.msi` and portable `.zip` for Windows x64 and arm64, and the `.AppImage`, `.deb` and `.tar.gz` for Linux x86_64 and aarch64 (on `ubuntu-22.04`, so the binary links against a glibc old enough for current LTS distributions). Each leg smoke-tests that its artefacts start and report the right version.
- `smoke-windows-arm64`, `verify-msi` and `verify-linux` install and run the artefacts on fresh runners, including an `.msi` upgrade over a previous install and a start test on the oldest and newest Ubuntu LTS.
- `publish` gathers the artefacts, writes `SHA256SUMS`, builds the notes from the version's `CHANGELOG.md` section (or the merged pull requests when there is none), and drafts a GitHub Release for a maintainer to review and publish by hand.

## Signing

The macOS and Windows packagers sign the artefacts when the signing secrets are set, and ad-hoc otherwise; the unsigned artefacts are listed in the release notes with how to open them past the first-run warning. See `tools/release/package-macos.sh` and `tools/release/package-windows.ps1`.
