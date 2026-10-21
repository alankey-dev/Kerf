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

`release.yml` runs on a pushed `v*` tag, or by hand (`workflow_dispatch`) for a dry run that keeps the artefacts, `SHA256SUMS` and notes as a workflow artifact instead of drafting a release. A dry run never signs and never sees the signing secrets.

- `check` verifies the tag matches `Cargo.toml`'s version (`tools/release/check-version.sh`), so a release cannot ship under a version the binary does not report. For a tag it also verifies that the tagged commit is reachable from `origin/main` (`git merge-base --is-ancestor`), so a tag on an unmerged commit builds nothing.
- `ci` calls `ci.yml` (`workflow_call`) on the tagged commit: format, Clippy and the tests on all three platforms, `cargo deny` on Linux, plus the timing gates. `build` waits for it.
- `build` builds every platform's artefacts on GitHub-hosted runners: the universal macOS `.dmg` on `macos-latest`, the `.msi` and portable `.zip` for Windows x64 and arm64, and the `.AppImage`, `.deb` and `.tar.gz` for Linux x86_64 and aarch64 (on `ubuntu-22.04`, so the binary links against a glibc old enough for current LTS distributions). Each leg smoke-tests that its artefacts start and report the right version. It holds no secrets, and the macOS and Windows legs do not use the Rust cache.
- `sign` (tag runs only) signs the macOS and Windows artefacts. See [Signing](#signing).
- `smoke-windows-arm64`, `verify-msi` and `verify-linux` install and run the artefacts on fresh runners, including an `.msi` upgrade over a previous install and a start test on the oldest and newest Ubuntu LTS.
- `publish` gathers the artefacts, writes `SHA256SUMS`, builds the notes from the version's `CHANGELOG.md` section (or the merged pull requests when there is none), and drafts a GitHub Release for a maintainer to review and publish by hand.

## Signing

Signing is separate from building, because every `build.rs` and proc-macro in the dependency graph (about 238 crates) can read the environment of `cargo build`.

1. The build jobs run `package-macos.sh <version>` and `package-windows.ps1 <version>`, which sign the macOS app ad hoc (Apple silicon will not run an unsigned binary) and ignore the signing variables. They upload the built `Kerf.app` (as a tar) and `kerf.exe` as `sign-input-<leg>`, beside the unsigned packages as `dist-<leg>`.
2. The `sign` job runs in the `release` environment and is the only job that references the signing secrets. It downloads those two artefacts, runs `package-macos.sh <version> --sign` or `package-windows.ps1 <version> -Target <triple> -Sign`, and replaces `dist-<leg>` with the signed set. No `cargo build` runs there.
   - macOS: signs the app with the Developer ID certificate and the hardened runtime, rebuilds the `.dmg`, signs it, notarises it and staples the ticket.
   - Windows: signs `kerf.exe`, rebuilds the `.zip` and `.msi` around it, and signs the `.msi`. Rebuilding the `.msi` needs `cargo-wix`, which reads the manifest through `cargo metadata` but compiles nothing. The script reads the certificate and takes it out of the environment before `cargo-wix` and the WiX tools run, so only `signtool` gets it.
3. Without the secrets, `sign` keeps the unsigned build and `unsigned-<leg>.txt` names the artefacts. The release notes list them, with how to open them past the first-run warning. A dry run lists every macOS and Windows artefact the same way.

The WiX Toolset binaries are downloaded and checked against a pinned SHA-256 on every packaging run, never trusted from `target\`. The linuxdeploy download in `package-linux.sh` is checked the same way.

### Repository settings (set by an admin, not by code)

The workflows assume these; none can be created from a workflow file.

- **A `release` environment** (Settings, Environments). Move `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`, `WINDOWS_CERT_PFX` and `WINDOWS_CERT_PASSWORD` here from repository secrets, so no other job can read them. Limit its deployment branches and tags to `v*` tags, and add required reviewers if a release should need a second approval. Until it exists, referencing it creates it empty and unprotected.
- **A tag ruleset** (Settings, Rules) restricting who can create `v*` tags, and blocking their update and deletion. The `check` job's main-ancestry test covers an unmerged commit, but not a maintainer tagging the wrong merged commit.
