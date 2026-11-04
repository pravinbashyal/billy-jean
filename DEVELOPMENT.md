# Developing Billy Jean

This document is for people who want to build, change or release Billy Jean. If you only want to use the app, see the [README](README.md).

## Requirements

- macOS 11 (Big Sur) or later
- A Rust stable toolchain, installed with [rustup](https://rustup.rs). The crate uses edition 2024, so Rust 1.85 or later is needed.
- Xcode Command Line Tools, which provide the linker: `xcode-select --install`

## Build and run from source

```bash
git clone https://github.com/pravinbashyal/billy-jean.git
cd billy-jean
cargo run --release
```

The first build downloads and compiles all dependencies, including GPUI, so it takes a while. Later builds are much faster.

To build the binary without running it, use the lockfile exactly as committed:

```bash
cargo build --release --locked
./target/release/billing
```

To run the tests:

```bash
cargo test --locked
```

The crate and binary are still named `billing`. Only the product name, "Billy Jean", changed. The GitHub repository is `pravinbashyal/billy-jean`.

## Inspection build (`gpui-mcp`)

The `gpui-mcp` feature is off by default. It publishes a semantic tree of the editor to the [`gpui-mcp`](https://crates.io/crates/gpui-mcp) stdio server and runs its bounded UI actions, so tools can inspect and drive the window during development:

```bash
cargo build --locked --features gpui-mcp
```

## Typing through `gpui-mcp` (`gpui-driver`)

The `TypeText` action needs two things: the patched `gpui-pre` fork and the `gpui-driver` feature, on top of `gpui-mcp`:

```bash
cargo build --features gpui-mcp,gpui-driver
```

This command leaves out `--locked` on purpose. Building with the patch active rewrites `Cargo.lock`, and `--locked` refuses that rewrite. After such a build, revert the change with `git checkout -- Cargo.lock` before committing. See the warning under "Optional local patch" below.

`gpui-driver` is off by default. It gates three calls in `src/editor.rs` (enabling the window driver, checking whether it is enabled, and inserting text into the focused field). Those calls exist only in the patched fork. On the stock `gpui-pre` 0.3.11 the feature does not compile, and without the feature `TypeText` returns the error `typing needs the gpui-driver feature (patched gpui-pre)`.

## Optional local patch (`.cargo/config.toml`)

To build `gpui-driver`, point Cargo at a local checkout of the patched `gpui-pre` crates. Create `.cargo/config.toml` in the repository root. It is git-ignored and must stay untracked:

```toml
[patch.crates-io]
gpui-pre = { path = "/path/to/gpui-pre-fork/gpui-pre-0.3.11" }
gpui-pre-linux = { path = "/path/to/gpui-pre-fork/gpui-pre-linux-0.3.11" }
gpui-pre-wgpu = { path = "/path/to/gpui-pre-fork/gpui-pre-wgpu-0.3.11" }
```

Without this file, every build uses the stock crates from crates.io, which is what the other commands in this document expect.

**Warning: do not commit a `Cargo.lock` changed while the patch is active.** With the patch active, Cargo rewrites `Cargo.lock` so the patched crates no longer have a `source` or `checksum`. On a machine without the patch, that lockfile makes `cargo build --locked` stop with "cannot update the lock file ... because --locked was passed". Before committing, check `git diff -- Cargo.lock`. If it removes `source` or `checksum` lines, revert it with `git checkout -- Cargo.lock`.

## Building the app bundle

`scripts/bundle-macos.sh <target-triple> <version>` builds `Billy Jean.app` and a `.dmg` in `dist/`, for example:

```bash
scripts/bundle-macos.sh aarch64-apple-darwin 1.0.0
```

The app is ad-hoc signed, not notarized, because the project has no Apple Developer account.

## Releasing

Versions and the changelog come from the commit messages, which follow [Conventional Commits](https://www.conventionalcommits.org):

- `feat: ...` gives a minor release (1.0.0 to 1.1.0).
- A breaking change gives a major release (1.0.0 to 2.0.0). That is a `!` after the type, such as `feat!: ...` or `fix!: ...`, or a `BREAKING CHANGE:` footer.
- `fix: ...`, and every other type that is not `feat:` or breaking, gives a patch release (1.0.0 to 1.0.1). That includes `docs:`, `chore:`, `refactor:`, `test:`, `ci:`, `build:` and `perf:`.

The convention is not enforced by a hook. Commits that do not follow it are left out of `CHANGELOG.md`.

To prepare a release, start on a clean, up-to-date `main` and run the release script. It needs [git-cliff](https://git-cliff.org), which you can install with `brew install git-cliff`:

```bash
scripts/release.sh               # computes the next version from the commits
scripts/release.sh --version 1.0.0   # sets the version explicitly, for the first release
```

The script creates a `release/vX.Y.Z` branch and makes one commit, `chore(release): vX.Y.Z`. That commit changes only `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`. It does not create a tag. Push the branch and open a pull request into `main`:

```bash
git push -u origin release/vX.Y.Z
gh pr create --base main --title "chore(release): vX.Y.Z"
```

When the release PR is merged, CI tags the merged commit `vX.Y.Z` and starts the release build automatically. That build produces the two `.dmg` files (arm64 and x86_64), publishes them to a GitHub Release, and updates the Homebrew Cask in [pravinbashyal/homebrew-billy-jean](https://github.com/pravinbashyal/homebrew-billy-jean). Nothing else needs to be done by hand.

The Cask update uses the `HOMEBREW_TAP_TOKEN` Actions secret, a fine-grained token with Contents write access to the tap only. If the token expires, releases still publish but the tap step fails, so rotate it then.
