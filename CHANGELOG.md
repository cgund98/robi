# Changelog

## [0.1.11](https://github.com/cgund98/robi/compare/v0.1.10...v0.1.11) (2026-10-10)


### Bug Fixes

* load directory as closed if no children found ([93519ce](https://github.com/cgund98/robi/commit/93519ce4d9778c1aa8c5ff7ae329b0050b7537b7))

## [0.1.10](https://github.com/cgund98/robi/compare/v0.1.9...v0.1.10) (2026-10-10)


### Features

* support ignored paths in doc viewer ([1a30d07](https://github.com/cgund98/robi/commit/1a30d07437de3fd1099f6e02d09c63cd5d65481f))

## [0.1.9](https://github.com/cgund98/robi/compare/v0.1.8...v0.1.9) (2026-10-06)


### Features

* add solid UI ([1d63f8b](https://github.com/cgund98/robi/commit/1d63f8ba85a055cfbfb798552118bd73796912a7))


### Bug Fixes

* composer not resetting, doc navigation ([a0cbcfc](https://github.com/cgund98/robi/commit/a0cbcfc68cd7ca8764874c02838d2b74e2174d3b))
* remove duplicate search entries for semantic ([fade985](https://github.com/cgund98/robi/commit/fade9858b3f2257c147146e57c68c17a405b9383))

## [0.1.8](https://github.com/cgund98/robi/compare/v0.1.7...v0.1.8) (2026-10-06)


### Bug Fixes

* use ubuntu 24 for releases ([802bf6e](https://github.com/cgund98/robi/commit/802bf6e237fc52e0ce9f81f4721f8ccbd65b03f4))

## [0.1.7](https://github.com/cgund98/robi/compare/v0.1.6...v0.1.7) (2026-10-06)


### Features

* support rejection reason in code review ([5e5bcb5](https://github.com/cgund98/robi/commit/5e5bcb541807273058a54063ec4cb28c068c2ba5))

## [0.1.6](https://github.com/cgund98/robi/compare/v0.1.5...v0.1.6) (2026-10-06)


### Features

* file uploads, attach docs to chat ([4b99f4d](https://github.com/cgund98/robi/commit/4b99f4d5b7fc21592585dbddad05acd18321dfd8))
* mcp logs, document search, mcp skill ([b3b3db1](https://github.com/cgund98/robi/commit/b3b3db1f7c809e9d6ef4a46ef427775492fc7aa0))

## [0.1.5](https://github.com/cgund98/robi/compare/v0.1.4...v0.1.5) (2026-10-05)


### Features

* anthropic models, compaction, blocking threads ([f687326](https://github.com/cgund98/robi/commit/f687326d780aacf6689baa2e2909142372522be7))
* implement anthropic models ([1a80ac0](https://github.com/cgund98/robi/commit/1a80ac07b7664b20c3e2f3cf866aabe2ac7e4fc4))


### Bug Fixes

* optimize review screen for one file at a time ([dcef8b4](https://github.com/cgund98/robi/commit/dcef8b435b670c73e88573af303e3c3debaeea09))
* review screen build types ([24d8a33](https://github.com/cgund98/robi/commit/24d8a334f09ff2005cb9dea5db0d894943ff7291))

## [0.1.4](https://github.com/cgund98/robi/compare/v0.1.3...v0.1.4) (2026-10-04)


### Bug Fixes

* support images from clipboard ([daf977c](https://github.com/cgund98/robi/commit/daf977c957a69aae21187dad26c0e8857ab0f3ef))

## [0.1.3](https://github.com/cgund98/robi/compare/v0.1.2...v0.1.3) (2026-10-04)


### Features

* Implement documents viewer ([7979acf](https://github.com/cgund98/robi/commit/7979acf214670549a0995fd3750be0ad7dd0ce19))
* support image inputs ([fe67246](https://github.com/cgund98/robi/commit/fe67246cdb454db6330c5f7716a3a767ec121960))


### Bug Fixes

* stream index updates via SSE ([62b7d9c](https://github.com/cgund98/robi/commit/62b7d9cce8852e2dc5c45d02ad51c5ef74828c5d))

## [0.1.2](https://github.com/cgund98/robi/compare/v0.1.1...v0.1.2) (2026-10-03)


### Bug Fixes

* publish ci uses correct dmg path ([a9c3a38](https://github.com/cgund98/robi/commit/a9c3a3878e9f41210d8b56ccd57735c25110cbc8))

## [0.1.1](https://github.com/cgund98/robi/compare/v0.1.0...v0.1.1) (2026-10-03)


### Features

* add semantic search ([a730793](https://github.com/cgund98/robi/commit/a730793cad721cb2dfacb99b578bb72a2be6b517))
* bundle API with tauri app ([1701bb3](https://github.com/cgund98/robi/commit/1701bb3a9b83c9f240abe29e92983bcab5616fb1))
* code review and subagents ([fb6c63b](https://github.com/cgund98/robi/commit/fb6c63be9cb9c94bac0c8f2f7ac1cfe5b1e683c2))
* generate tauri boilerplate for frontend ([ea88570](https://github.com/cgund98/robi/commit/ea88570f8f6ec0ae9a9f7b5d6a5890392adfa218))
* implement agent runtime and chat messages persistence ([06c3a52](https://github.com/cgund98/robi/commit/06c3a52158e87217498a21057f37d18729de0975))
* implement core agent loop ([1bdb56f](https://github.com/cgund98/robi/commit/1bdb56f0e39ed66c0aa2506843848c12d2f6912e))
* implement edit files tools ([85833b1](https://github.com/cgund98/robi/commit/85833b16bab8611963eacdc36242737762110b70))
* implement shell command and mcp compaction ([1f63b97](https://github.com/cgund98/robi/commit/1f63b976c0f52f6ed53983c7a449e6b354549d40))
* opencode-go as first model provider ([1019a4f](https://github.com/cgund98/robi/commit/1019a4f9aea98ee87dc602faf0051ef6e3f71d1c))
* organize sessions into workspaces ([8fa24fc](https://github.com/cgund98/robi/commit/8fa24fce67fe4ec9ec1082e8d03bbf611f745184))
* restructure documentation ([2086595](https://github.com/cgund98/robi/commit/2086595dd4e27cc57fd3d0c4f47091ba68f496ea))
* support LSP tools ([c593660](https://github.com/cgund98/robi/commit/c59366038657a077c256d0e9c91bd97960515976))
* support MCP ([10fc70b](https://github.com/cgund98/robi/commit/10fc70b9a2c38d8c94beb84a3fa5061c933bc311))
* support read tools ([dd4c391](https://github.com/cgund98/robi/commit/dd4c391171b0b7a86ddba4adfa010afdc81a3ac3))
* support read_code with tree parsing ([ffcfc9c](https://github.com/cgund98/robi/commit/ffcfc9c71367a889f929fb226c9a4b3463d66a16))


### Bug Fixes

* ensure fetch ordering for UI state updates ([fcc9925](https://github.com/cgund98/robi/commit/fcc9925031c54845e7e16771f7af1aa1c8535b7a))
* implement sandboxed shell command ([2ca3fc9](https://github.com/cgund98/robi/commit/2ca3fc95aaae472e67a78f60191d66532a6e118e))
* refactor robi crate into 4 modules ([afc1e79](https://github.com/cgund98/robi/commit/afc1e79bdc416f52c4de23abbdbc0ab635389fd0))
* remove scratch ([4252564](https://github.com/cgund98/robi/commit/425256404c507542bf54bdadfe9e484fb2f3e804))
