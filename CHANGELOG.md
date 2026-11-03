# Changelog

## [1.0.1](https://github.com/gcaudan/herdr-explorer/compare/v1.0.0...v1.0.1) (2026-11-03)


### Fixes

* **diff:** saturate line numbers of a hunk starting past i64 ([#37](https://github.com/gcaudan/herdr-explorer/issues/37)) ([4cbe14b](https://github.com/gcaudan/herdr-explorer/commit/4cbe14b92e3e9f0428af95eeb3df4b42b1d30e9b))
* **fetch-bin:** hash with .NET, not Get-FileHash, missing when herdr starts from pwsh ([#32](https://github.com/gcaudan/herdr-explorer/issues/32)) ([ad41ad3](https://github.com/gcaudan/herdr-explorer/commit/ad41ad37dd1da9212de2d8cd7c069caf68c736fb))
* **herdr-preview:** resolve the peek binary path with a python one-liner ([#36](https://github.com/gcaudan/herdr-explorer/issues/36)) ([ee92356](https://github.com/gcaudan/herdr-explorer/commit/ee923565854ffa0fe8a8421adad26625e67dc3ac))
* keep the explorer's width when a pane is stacked in its column ([#31](https://github.com/gcaudan/herdr-explorer/issues/31)) ([44feb40](https://github.com/gcaudan/herdr-explorer/commit/44feb406e4f56b60a2912fecdd5ace23b0c1501c))
* never close a pane the user renamed with one of the plugin's labels ([#33](https://github.com/gcaudan/herdr-explorer/issues/33)) ([2526323](https://github.com/gcaudan/herdr-explorer/commit/25263234a219fe1490c6ab58bb48b2abb3b8de83))

## [1.0.0](https://github.com/gcaudan/herdr-explorer/compare/v0.8.0...v1.0.0) (2026-10-31)


### ⚠ BREAKING CHANGES

* the plugin needs its binary in `bin/`, fetched by the manifest's build step for Linux and macOS (x86_64, arm64) and Windows; elsewhere, build it with `scripts/dev-build.sh`. git features need git 2.31 or later (an older git counts as missing).

### Features

* rewrite the plugin in Rust ([#29](https://github.com/gcaudan/herdr-explorer/issues/29)) ([60f48e2](https://github.com/gcaudan/herdr-explorer/commit/60f48e262ac173cf5f2666db484d7843a6a21239))

## [0.8.0](https://github.com/gcaudan/herdr-explorer/compare/v0.7.1...v0.8.0) (2026-10-25)


### ⚠ BREAKING CHANGES

* partial staging, diff scopes, notes and source control in tabs for 0.8.0 ([#27](https://github.com/gcaudan/herdr-explorer/issues/27))

### Features

* partial staging, diff scopes, notes and source control in tabs for 0.8.0 ([#27](https://github.com/gcaudan/herdr-explorer/issues/27)) ([1962de8](https://github.com/gcaudan/herdr-explorer/commit/1962de8a3874d8ed703c67caede89d7d70af7605))


### Fixes

* **viewed:** hash a file again when it changed within 2 s of its last hash ([#28](https://github.com/gcaudan/herdr-explorer/issues/28)) ([d3a873e](https://github.com/gcaudan/herdr-explorer/commit/d3a873ecb1a4237f2535e3392707fcc8dcb7a189))

## [0.7.1](https://github.com/gcaudan/herdr-explorer/compare/v0.7.0...v0.7.1) (2026-10-21)


### Fixes

* harden 0.7.0 after its code review ([#23](https://github.com/gcaudan/herdr-explorer/issues/23)) ([8d75725](https://github.com/gcaudan/herdr-explorer/commit/8d757250eb0f81372194af7588abf37ae8f2fb94))

## [0.7.0](https://github.com/gcaudan/herdr-explorer/compare/v0.6.0...v0.7.0) (2026-10-21)


### Features

* add review notes, preview tabs, in-place editing and git operations ([#21](https://github.com/gcaudan/herdr-explorer/issues/21)) ([fcbc8ff](https://github.com/gcaudan/herdr-explorer/commit/fcbc8ff0d9cd8c92b2c1a3e848c60c51e2698123))

## [0.6.0](https://github.com/gcaudan/herdr-explorer/compare/v0.5.1...v0.6.0) (2026-10-16)


### Features

* redesign the explorer, source control and shared UI ([#19](https://github.com/gcaudan/herdr-explorer/issues/19)) ([8d6db93](https://github.com/gcaudan/herdr-explorer/commit/8d6db934b4c5cb47c5ffc88059ec7159ceef091f))

## [0.5.1](https://github.com/gcaudan/herdr-explorer/compare/v0.5.0...v0.5.1) (2026-10-14)


### Fixes

* address Linux test findings and split the docs ([#15](https://github.com/gcaudan/herdr-explorer/issues/15)) ([3403ef1](https://github.com/gcaudan/herdr-explorer/commit/3403ef13f0633c70686586e051966168a8e8ae17))

## [0.5.0](https://github.com/gcaudan/herdr-explorer/compare/v0.4.1...v0.5.0) (2026-10-14)


### Features

* support installing from the herdr plugin index ([#12](https://github.com/gcaudan/herdr-explorer/issues/12)) ([b66f61b](https://github.com/gcaudan/herdr-explorer/commit/b66f61b5a72d337cc749c56e14494e3f51dc018d))
