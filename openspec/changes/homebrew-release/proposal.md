# Proposal

## Why

The invoice builder can only be run with `cargo run` from a private checkout on one machine. To ship it as "Billy Jean", installable with `brew install --cask billy-jean`, the repo needs to be publishable: no client or personal data in its history, no build-time dependency on directories that only exist on the developer's laptop, a licence and README, a versioned release pipeline that produces a launchable `.app`, and a Homebrew tap that tracks it. `pravinbashyal/pdf-tools` already does all of this and is the model here. This change is captured now and applied after `add-invoice-builder` merges to `main`, because the app is not usable until PDF export (that change's tasks 9.x-11.x) lands.

## What Changes

- **BREAKING (repo history):** publish a brand-new repository with a single initial commit instead of the old history. It contains the code and `openspec/` only, with the reference client invoice, `.claude/` and `dist/` left out. Replace the real client and sender names, addresses and IBAN with made-up values in every file (the IBAN becomes a checksum-valid example). The old repository stays local as a private backup.
- Add `.claude/`, `dist/`, `.cargo/config.toml` and `output-test` to `.gitignore` so the local dev patch, build output and tooling stay local. `openspec/` is tracked and published.
- Move the dev-only `[patch.crates-io]` block (the three absolute `/Users/pravinbashyal/project/gpui-pre-0.3.11-agent/...` paths) out of `Cargo.toml` into an untracked `.cargo/config.toml`. Remove the `gpui-agent` path dependency, the `gpui-agent` feature and its `#[cfg(feature = "gpui-agent")]` call site. The crates.io-based `gpui-mcp` feature stays. Verify the release build compiles against stock `gpui-pre =0.3.11`.
- Product name becomes **Billy Jean**: `Billy Jean.app`, bundle ID `com.pravinbashyal.billy-jean`, cask token `billy-jean`, tap `pravinbashyal/homebrew-billy-jean`. The crate, binary and per-user config directory keep their current `billing` names (see design.md D4).
- Set `Cargo.toml` `version` to `1.0.0` for the first public release and add `license = "MIT"`, `description` and `repository` fields.
- Add an MIT `LICENSE` and a `README.md` covering what the app does, building from source, the Homebrew install, and the unsigned-build Gatekeeper override.
- Add a bundling script that turns a release binary into `Billy Jean.app` (Info.plist, binary, `OFL.txt` for the embedded Noto Sans font), re-signs it ad-hoc, and wraps it in a `.dmg`.
- Add `.github/workflows/release.yml`. On a `v*` tag or a manual run with a `tag` input, it:
  - checks that the tag matches the `Cargo.toml` version;
  - builds arm64 and x86_64 `.dmg`s on a macOS runner and verifies their signatures;
  - publishes them to a GitHub Release with notes on getting past Gatekeeper;
  - updates the Cask in the tap repo.
- Add a release-PR flow driven by Conventional Commits:
  - A local `scripts/release.sh` uses `git-cliff` to compute the next version and write `CHANGELOG.md`, then bumps `Cargo.toml`/`Cargo.lock` on a `release/vX.Y.Z` branch.
  - `.github/workflows/tag-release-pr.yml` tags `main` when that PR merges and starts `release.yml`.
- Create the `pravinbashyal/homebrew-billy-jean` tap repo with `Casks/billy-jean.rb` (per-architecture `url`/`sha256`, unsigned-build caveat) and a short README.

Out of scope (deferred, not rejected): Apple code signing and notarization (no Apple Developer account), submission to the official `homebrew/cask`, a custom app icon, a window title, Linux/Windows builds, CONTRIBUTING.md and issue templates.

## Capabilities

### New Capabilities
- `open-source-licensing`: what makes the repo publishable. It covers a LICENSE that matches `Cargo.toml`, a README that accurately describes building and installing, a public history with no client or personal data or internal workflow files, and a manifest that builds from a fresh clone without machine-local paths.
- `release-packaging`: what a release is and how it comes to exist. It covers the Conventional-Commits version bump and changelog on a release PR, tagging on merge, a version-checked CI build of unsigned arm64 and x86_64 `Billy Jean.app` `.dmg`s, and their publication to a GitHub Release.
- `homebrew-distribution`: installing through the personal tap. `brew tap pravinbashyal/billy-jean` plus `brew install --cask billy-jean` should resolve to a launchable `Billy Jean.app` for the host architecture, and the Cask should stay current automatically on each release.

### Modified Capabilities
_None. `openspec/specs/` is empty and `add-invoice-builder`'s capabilities (`invoice-editing`, `pdf-export`) are not changed by this work._

## Impact

- `Cargo.toml`: version, metadata, removal of `[patch.crates-io]`, the `gpui-agent` dependency and the `gpui-agent` feature. `Cargo.lock` is regenerated against stock crates.io sources.
- `src/main.rs`: removal of the `gpui-agent` cfg call site. Test fixtures in `src/app_state.rs`, `src/editor.rs`, `src/iban.rs`, `src/identity_store.rs` and `src/lib.rs` swap real names, addresses and IBAN for made-up ones. No behaviour changes.
- New files: `LICENSE`, `README.md`, `CHANGELOG.md` (generated), `cliff.toml`, `scripts/bundle-macos.sh`, `scripts/release.sh`, `packaging/Info.plist`, `.github/workflows/release.yml`, `.github/workflows/tag-release-pr.yml`. `.gitignore` gets the edits above.
- Local-only: `.cargo/config.toml` holds the dev patch. `openspec/` is published with the code (anonymized).
- External: new public GitHub repos `pravinbashyal/billing` and `pravinbashyal/homebrew-billy-jean`, and a `HOMEBREW_TAP_TOKEN` Actions secret (a fine-grained PAT with write access to Contents on the tap only).
- Dev tooling: `gpui-agent` is gone, so `cargo build --features gpui-agent` no longer exists. `gpui-mcp` is the remaining inspection path. The empty `gpui-driver` feature stays (PO decision): with it and the untracked `.cargo/config.toml` patch, typing works through the patched fork; without it, the TypeText action returns an error. Release and Homebrew builds use neither feature.
