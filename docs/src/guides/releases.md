# Releases

A push to `main` runs [release-please](https://github.com/googleapis/release-please). It opens one pull request that bumps the app version and updates `CHANGELOG.md`. Merging that pull request tags `vX.Y.Z` and publishes a GitHub release. The same workflow then builds the desktop bundles and attaches them, with a SHA-256 checksum, to that release. It produces an unsigned Apple Silicon disk image (`.dmg`) for macOS, and an AppImage and a Debian package (`.deb`) for Linux. The Linux bundles are built on Ubuntu 24.04, because the prebuilt ONNX Runtime archive needs glibc 2.38. They need that glibc to run. The builds run in the release workflow because a release created with the repository token does not start another workflow. Windows bundles are not built.

The version is one number for the whole app. The release pull request writes it in three places:

| File | Field |
|---|---|
| `Cargo.toml` | `workspace.package.version` |
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |

Crates take the workspace version, so they are not listed separately. `openapi/openapi.json` is generated; the next `make openapi-spec` picks up the new version.

Commits on `main` use [Conventional Commits](https://www.conventionalcommits.org/). While the version is below `1.0.0`, `fix` and `feat` both open a patch (`0.1.1`), and `feat!` or `BREAKING CHANGE` opens a minor (`0.2.0`). Other types are omitted from the changelog unless they carry a breaking marker.
