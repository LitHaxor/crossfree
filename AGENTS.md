# AGENTS.md

Guidance for coding agents (and people) working on crossfree.

crossfree plays Windows games on Apple silicon Macs. A SwiftUI app that feels
like a game console drives a Rust command-line tool, which runs Windows
programs with arm64 Wine + FEX ([mach9](https://github.com/athei/mach9)'s
engine, shipped inside the app). There is no VM and no Rosetta.

## Map

| Path | What |
|---|---|
| `app/` | The macOS app (SwiftUI, Swift package). `Store.swift` holds state and runs the CLI; one file per screen. |
| `crates/launcher/` | The `crossfree` CLI: engine, prefixes, launching with per-game rules, Start Menu apps, icons, Steam artwork, Dock names (`dock.rs`). |
| `crates/d3d9-bytecode/`, `shader-ir/`, `msl-emitter/` | A Direct3D 9 shader → Metal Shading Language translator (parked research, kept tested). |
| `crates/wine-test-corpus/` | Test helper: the shaders in Wine's D3D9 conformance tests. |
| `tools/` | `build-app.sh` (makes `target/crossfree.app`), `make-icon.swift`, `store-logos.py`. |
| `docs/` | `ARCHITECTURE.md` (the plan and its decisions), spikes, README images. |
| `research/` | Local only (git-ignored): engines, third-party sources. |

## Build and test

```sh
cargo test --workspace                       # Rust (CI pins 1.94.1)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
swift test --package-path app                # app logic, against a fake CLI
swift format lint --strict -r app/Sources app/Tests app/Package.swift tools/make-icon.swift
CROSSFREE_ENGINE=/path/to/mach9/wine ./tools/build-app.sh && open target/crossfree.app
```

- Wine's D3D9 tests are not committed. `CF_WINE_D3D9_TESTS=<dir>` points at
  them; `CF_REQUIRE_WINE_CORPUS=0` skips the corpus tests (they fail when
  missing on CI). CI fetches them from CrossOver's LGPL sources.
- With only Apple's Command Line Tools, swift-testing needs
  `-Xswiftc -plugin-path -Xswiftc /Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing`.
- Every change comes with tests; keep all of the above green.

## Code

- **Rust:** edition 2024, `unsafe_code` forbidden, public items documented
  (`missing_docs`). 120 columns (`rustfmt.toml`). Few dependencies: add one
  only for a clear reason. Errors are `launcher::Error`, with the path and
  what was being done.
- **Swift:** Swift 5 language mode; `ObservableObject` (the Command Line
  Tools have no macro plugins, so no `@Observable`). Colours, animations and
  components come from `Theme.swift` and `Cards.swift`.
- Comments say why, not what. Tests describe behaviour in their names.

## The engine

- `wine.app` (the signed loader) must stay byte-for-byte as mach9 ships it:
  its signature carries the entitlements Wine needs. Never re-sign it, edit
  its `Info.plist` or move it out of `lib/wine/aarch64-unix/`.
- `tools/build-app.sh` copies the engine to `Contents/SharedSupport/engine`
  (APFS clone).
- Dock names: each game runs from a mirror of the engine with the loader
  copied as `<Game>.app` (see `crates/launcher/src/dock.rs`). Copies, never
  hard links; no `wine.app` in a mirror (mach9's ntdll would use it).

## The app's look and behaviour

- A console (PlayStation + SteamOS), not a Wine manager: full-window art,
  glass cards, spring animations, keyboard and gamepad parity for every action.
- Never show Wine prefixes ("bottles") or the engine. Each installer gets a
  prefix of its own automatically; the engine is mentioned only when broken.
- No filler: no welcome screens, marketing lines, status chips or counts.
  One way to do each thing.
- Tabs are nouns: Games, Directory, Settings.

## Commits and releases

- One logical change per commit, imperative subject (`launcher: …`,
  `app: …`, `ci: …`), body says why. No AI attribution lines.
- CI (`.github/workflows/ci.yml`) must pass. Tag `vX.Y.Z` to release:
  `release.yml` builds the app with the engine inside, packs a DMG and
  publishes it.
