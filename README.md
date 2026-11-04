# Billy Jean

Billy Jean is a small native desktop app for macOS that writes invoices as PDF files.

The window has two parts. The left side is a form with the panels Sender, Bill to, Invoice details, Line items, Totals, Notes and Bank details. The right side is a live preview of the PDF, which updates as you type. **Export PDF** writes the PDF to a file you choose, after checking that the required fields are filled in, there is at least one line item, and the IBAN is valid.

- Line totals and invoice totals (net, VAT, gross) are calculated for you.
- The VAT rate defaults to 0%. At 0% the invoice carries the German §19 UStG (small business) note. Any other rate recalculates VAT and gross and replaces that note.
- The PDF is generated inside the app. No external tools such as LaTeX or Ghostscript are needed.
- Sender and bank details are saved between runs in `~/Library/Application Support/com.pravinbashyal.billing/identity.json`, so they are pre-filled next time. Invoices themselves are not saved, and there is no invoice history, client address book, automatic numbering or currency other than EUR.

## Requirements

- macOS 11 (Big Sur) or later
- A Rust stable toolchain, installed with [rustup](https://rustup.rs). The crate uses edition 2024, so Rust 1.85 or later is needed.
- Xcode Command Line Tools, which provide the linker: `xcode-select --install`

## Build and run from source

```bash
git clone https://github.com/pravinbashyal/billing.git
cd billing
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

The crate and binary are still named `billing`. Only the product name, "Billy Jean", changed.

## Developer notes

### Inspection build (`gpui-mcp`)

The `gpui-mcp` feature is off by default. It publishes a semantic tree of the editor to the [`gpui-mcp`](https://crates.io/crates/gpui-mcp) stdio server and runs its bounded UI actions, so tools can inspect and drive the window during development:

```bash
cargo build --locked --features gpui-mcp
```

### Typing through `gpui-mcp` (`gpui-driver`)

The `TypeText` action needs two things: the patched `gpui-pre` fork and the `gpui-driver` feature, on top of `gpui-mcp`:

```bash
cargo build --features gpui-mcp,gpui-driver
```

This command leaves out `--locked` on purpose. Building with the patch active rewrites `Cargo.lock`, and `--locked` refuses that rewrite. After such a build, revert the change with `git checkout -- Cargo.lock` before committing. See the warning under "Optional local patch" below.

`gpui-driver` is off by default. It gates three calls in `src/editor.rs` (enabling the window driver, checking whether it is enabled, and inserting text into the focused field). Those calls exist only in the patched fork. On the stock `gpui-pre` 0.3.11 the feature does not compile, and without the feature `TypeText` returns the error `typing needs the gpui-driver feature (patched gpui-pre)`.

### Optional local patch (`.cargo/config.toml`)

To build `gpui-driver`, point Cargo at a local checkout of the patched `gpui-pre` crates. Create `.cargo/config.toml` in the repository root. It is git-ignored and must stay untracked:

```toml
[patch.crates-io]
gpui-pre = { path = "/path/to/gpui-pre-fork/gpui-pre-0.3.11" }
gpui-pre-linux = { path = "/path/to/gpui-pre-fork/gpui-pre-linux-0.3.11" }
gpui-pre-wgpu = { path = "/path/to/gpui-pre-fork/gpui-pre-wgpu-0.3.11" }
```

Without this file, every build uses the stock crates from crates.io, which is what the other commands in this README expect.

**Warning: do not commit a `Cargo.lock` changed while the patch is active.** With the patch active, Cargo rewrites `Cargo.lock` so the patched crates no longer have a `source` or `checksum`. On a machine without the patch, that lockfile makes `cargo build --locked` stop with "cannot update the lock file ... because --locked was passed". Before committing, check `git diff -- Cargo.lock`. If it removes `source` or `checksum` lines, revert it with `git checkout -- Cargo.lock`.

## Install via Homebrew

Billy Jean is distributed through a personal (non-official) Homebrew tap, [pravinbashyal/homebrew-billy-jean](https://github.com/pravinbashyal/homebrew-billy-jean), which tracks this project's [GitHub Releases](https://github.com/pravinbashyal/billing/releases).

```bash
brew tap pravinbashyal/billy-jean
brew install --cask billy-jean
```

This installs `Billy Jean.app` into `/Applications`. The tap provides the arm64 build on Apple silicon and the x86_64 build on Intel. As with the direct `.dmg` download, the build is unsigned and non-notarized, so first launch may need a Gatekeeper override (see [Unsigned build](#unsigned-build) below).

To upgrade or remove:

```bash
brew update
brew upgrade --cask billy-jean
# or
brew uninstall --cask billy-jean
brew untap pravinbashyal/billy-jean
```

A plain `brew uninstall` keeps your saved sender and bank details. `brew uninstall --cask --zap billy-jean` also deletes `~/Library/Application Support/com.pravinbashyal.billing`.

## Unsigned build

Tagged releases (see [Releases](https://github.com/pravinbashyal/billing/releases)) publish a `.dmg` for each architecture, built by CI. The build is **not code-signed or notarized**, because the project has no Apple Developer account. If you download the `.dmg` directly, or install through Homebrew, and macOS Gatekeeper reports the app as "damaged" or refuses to open it:

- On macOS 15 (Sequoia) and later, open **System Settings → Privacy & Security**, scroll down to the notice about the blocked app, click **Open Anyway**, and confirm. This is only needed once.
- On older macOS versions, right-click (or Control-click) **Billy Jean** in `/Applications` and choose **Open**.
- If neither shows an override option (the "move to Trash" variant of the warning), run this in Terminal instead:

  ```bash
  xattr -dr com.apple.quarantine "/Applications/Billy Jean.app"
  ```

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

When the release PR is merged, CI tags the merged commit `vX.Y.Z` and starts the release build automatically. That build produces the two `.dmg` files, publishes them to a GitHub Release, and updates the Homebrew Cask. Nothing else needs to be done by hand.

## License

Billy Jean is released under the MIT License. See [LICENSE](LICENSE). The bundled Noto Sans fonts are licensed under the SIL Open Font License, see `assets/fonts/OFL.txt`.
