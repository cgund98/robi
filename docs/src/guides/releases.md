# Releases

A push to `main` runs [release-please](https://github.com/googleapis/release-please). It opens one pull request that bumps the app version and updates `CHANGELOG.md`. Merging that pull request tags `vX.Y.Z` and publishes a GitHub release. The same workflow then builds an unsigned Apple Silicon disk image and attaches it, with a SHA-256 checksum, to that release. The build runs in the release workflow because a release created with the repository token does not start another workflow. Linux and Windows bundles are not built.

The version is one number for the whole app. The release pull request writes it in three places:

| File | Field |
|---|---|
| `Cargo.toml` | `workspace.package.version` |
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |

Crates take the workspace version, so they are not listed separately. `openapi/openapi.json` is generated; the next `make openapi-spec` picks up the new version.

Commits on `main` use [Conventional Commits](https://www.conventionalcommits.org/). While the version is below `1.0.0`, `fix` and `feat` both open a patch (`0.1.1`), and `feat!` or `BREAKING CHANGE` opens a minor (`0.2.0`). Other types are omitted from the changelog unless they carry a breaking marker.
