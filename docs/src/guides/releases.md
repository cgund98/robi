# Releases

A push to `main` runs [release-please](https://github.com/googleapis/release-please). It opens one pull request that bumps the app version and updates `CHANGELOG.md`. Merging that pull request tags `vX.Y.Z` and publishes a GitHub release. The same workflow then builds everything and attaches it, with a SHA-256 checksum, to that release. The builds run in the release workflow because a release created with the repository token does not start another workflow.

Each release carries:

| Asset | What it is |
|---|---|
| A `.dmg` | The desktop app for Apple Silicon macOS. Unsigned; ad-hoc signed only. |
| An `.AppImage` | The desktop app for Linux, carrying its own runtime dependencies. |
| A `.deb` | The desktop app for Debian and Ubuntu. |
| `robi-vX.Y.Z-aarch64-apple-darwin.tar.gz` | The terminal app for arm64 macOS. |
| `robi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | The terminal app for x86_64 Linux. |
| `checksums.txt` | SHA-256 of every asset above. |

Windows is not built.

## What the builds need

The desktop bundles are built on Ubuntu 24.04, which sets their glibc floor at 2.39. A prebuilt ONNX Runtime archive used to force that; the dependency is gone, and the row stays until a 22.04 build of the bundles is tested. The `.deb` needs `libwebkit2gtk-4.1` at runtime; the AppImage carries its own.

The terminal tarballs are built on Ubuntu 22.04 instead, so their glibc floor is 2.35 and one file runs on 22.04 and 24.04 both. They need no webview, no front end, and no Node: the job builds `-p robi --features tui --bin robi` and nothing else. Each tarball holds one directory, `robi-vX.Y.Z-<target>/`, with the `robi` binary and `LICENSE`. The binary is stripped, and on macOS re-signed ad-hoc after stripping.

Both tarballs run `robi --version` and compare it with the tag before they are uploaded, so a release that ships a binary whose version disagrees with the tag fails instead of publishing.

## Install the terminal app

```sh
tar -xzf robi-vX.Y.Z-aarch64-apple-darwin.tar.gz
sudo install -m 0755 robi-vX.Y.Z-aarch64-apple-darwin/robi /usr/local/bin/robi
```

On macOS, delete the quarantine flag first, because the binary is not notarised:

```sh
xattr -d com.apple.quarantine robi-vX.Y.Z-aarch64-apple-darwin/robi
```

On Linux, the default sandbox runs commands through `bubblewrap`; without it, a sandboxed command is refused. See [the terminal app](terminal.md).

## The version

The version is one number for the whole app. The release pull request writes it in three places:

| File | Field |
|---|---|
| `Cargo.toml` | `workspace.package.version` |
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |

Crates take the workspace version, so they are not listed separately. `openapi/openapi.json` is generated; the next `make openapi-spec` picks up the new version.

Commits on `main` use [Conventional Commits](https://www.conventionalcommits.org/). While the version is below `1.0.0`, `fix` and `feat` both open a patch (`0.1.1`), and `feat!` or `BREAKING CHANGE` opens a minor (`0.2.0`). Other types are omitted from the changelog unless they carry a breaking marker.
