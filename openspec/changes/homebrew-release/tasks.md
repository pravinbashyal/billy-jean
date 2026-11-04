# Tasks

Precondition: `add-invoice-builder` has merged to `main`. Do groups 1-5 on a branch off `main` and merge them before group 6. Group 6 contains irreversible, Product-Owner-involved steps (publication, first real release), and each step stays its own checkbox for that reason.

## 1. Buildable, publishable manifest

- [x] 1.1 Remove `[patch.crates-io]`, the `gpui-agent` optional path dependency, the `gpui-agent` feature, and the `#[cfg(feature = "gpui-agent")] gpui_agent::init(cx);` call in `src/main.rs` (design D5). Move the patch block verbatim into an untracked `.cargo/config.toml`. Set `version = "1.0.0"` and add `license = "MIT"`, `description` and `repository = "https://github.com/pravinbashyal/billing"` to `[package]`. Regenerate `Cargo.lock` with the patch inactive, so every `gpui-pre*` entry has a crates.io `source`. Update `.gitignore` with `.claude/`, `openspec/*`, `!openspec/config.yaml` and `.cargo/config.toml`. Verify in a fresh `git clone` into a scratch directory, where the sibling patch dir is not referenced:
  - `cargo build --release --locked`, `cargo test --locked` and `cargo build --locked --features gpui-mcp` all succeed against stock `gpui-pre =0.3.11`;
  - `grep -n 'path *=' Cargo.toml` finds nothing;
  - the release binary launches and the editor shows visible text, not only panel borders (see the font-kit note in project memory).

  If the stock build or rendering fails, stop and escalate. That reopens PO decision Q2.
- [x] 1.2 Add an MIT `LICENSE` ("Copyright (c) 2026 pravinbashyal") and a `README.md` naming the app "Billy Jean". Cover what it does, requirements (macOS 11+, Rust stable), build and run from source (`cargo run --release`), and a developer section: the `gpui-mcp` feature, and the optional untracked `.cargo/config.toml` patch with its warning not to commit a `Cargo.lock` changed while it is active. Verify that `LICENSE` and `Cargo.toml` both say MIT, and that each build/run command in the README runs as written in the scratch clone.

## 2. App bundle and disk images

- [x] 2.1 Add `packaging/Info.plist` (keys per design D2: "Billy Jean", `com.pravinbashyal.billy-jean`, `CFBundleExecutable` `billing`, `LSMinimumSystemVersion` 11.0, version placeholders) and `scripts/bundle-macos.sh <triple> <version>`. The script builds with `--locked` and `MACOSX_DEPLOYMENT_TARGET=11.0`, assembles `Billy Jean.app` with `Resources/OFL.txt`, ad-hoc signs as the last step, and writes `dist/Billy-Jean-<version>-<arch>.dmg` (`arm64` or `x86_64`) containing the app and an Applications symlink. Ignore `dist/`. Verify by running it locally for both `aarch64-apple-darwin` and `x86_64-apple-darwin` with version `1.0.0`:
  - `codesign --verify --deep --strict` passes on both apps;
  - `lipo -archs` on each `Contents/MacOS/billing` shows the right single architecture;
  - `plutil -extract CFBundleShortVersionString raw` gives `1.0.0`;
  - `Contents/Resources/OFL.txt` exists;
  - each `.dmg` mounts with `hdiutil attach` and shows `Billy Jean.app`;
  - the arm64 app copied to `/Applications` launches from Finder with visible text and the menu-bar name "Billy Jean", and still finds the identity already saved in `~/Library/Application Support/com.pravinbashyal.billing`.

## 3. Versioning and release PR flow

- [x] 3.1 Add `cliff.toml` (Conventional Commits preset, `filter_unconventional = true`) and `scripts/release.sh [--version X.Y.Z]` per design D6. It requires a clean, up-to-date `main`, refuses a `Cargo.lock` with a `gpui-pre*` entry lacking a crates.io `source`, computes the version (explicit or `git cliff --bumped-version`), creates `release/vX.Y.Z`, sets the `[package]` version, runs `cargo update --workspace --offline`, prepends `CHANGELOG.md`, and makes one `chore(release): vX.Y.Z` commit without tagging. Add a "Releasing" section to `README.md` covering the Conventional Commits convention, the script, the release PR, and that merging tags and builds automatically. Verify in a scratch clone with a throwaway `v1.0.0` tag and one `feat:` commit after it:
  - the script yields `1.1.0` in `Cargo.toml` and `Cargo.lock`, a `1.1.0` section in `CHANGELOG.md`, exactly one new commit touching only those three files, and no new tag;
  - `--version 1.0.0` on an untagged clone yields `1.0.0`;
  - a hand-polluted lockfile (a `gpui-pre` entry with its `source` line removed) makes the script exit non-zero before changing anything.

## 4. CI workflows

- [x] 4.1 Add `.github/workflows/tag-release-pr.yml` and `.github/workflows/release.yml` per design D7, mirroring `/Users/pravinbashyal/project/pdf-tools/.github/workflows/`:
  - tag-on-merge for `release/*` PRs, failing if the tag exists, then `gh workflow run release.yml -f tag=...`;
  - the release job: `v*`/`workflow_dispatch` triggers, the `cargo metadata` version check, both-target bundling via `scripts/bundle-macos.sh`, strict `codesign --verify`, and `softprops/action-gh-release@v2` with `fail_on_unmatched_files` and Billy Jean Gatekeeper notes (macOS 15+ "Open Anyway", Control-click then Open on older versions, `xattr -dr com.apple.quarantine "/Applications/Billy Jean.app"` fallback);
  - the tap step: check out `pravinbashyal/homebrew-billy-jean` with `HOMEBREW_TAP_TOKEN`, update `version` and the `on_arm`/`on_intel` `sha256` in `Casks/billy-jean.rb` with the pdf-tools Python regex (exactly one replacement each, otherwise fail), and commit/push only on change.

  Verify:
  - `actionlint` reports no errors on both files;
  - the Python update snippet, extracted and run locally against a copy of the task 5.1 Cask with dummy hashes, changes exactly the version and both hashes, and fails on a Cask missing an `on_intel` block;
  - the version-check step, run locally as a shell snippet, fails for tag `v9.9.9` against `1.0.0`.

## 5. Homebrew tap

- [x] 5.1 Create the tap working copy at `/Users/pravinbashyal/project/homebrew-billy-jean` (`git init`, not yet pushed) with `Casks/billy-jean.rb` per design D8 (version `1.0.0`, `on_arm`/`on_intel` `sha256` then `url` pointing at `https://github.com/pravinbashyal/billing/releases/download/v#{version}/Billy-Jean-#{version}-<arch>.dmg`, `depends_on :macos` (an explicit floor is flagged redundant by `brew style`; `LSMinimumSystemVersion` 11.0 enforces it), `app "Billy Jean.app"`, the unsigned/unofficial caveat, and a `zap` of `~/Library/Application Support/com.pravinbashyal.billing`), plus a short README with tap, install, upgrade, uninstall and untap commands. Add an "Install via Homebrew" section and an "Unsigned build" section to the app's `README.md`. For now, use the `sha256` of the task 2.1 local dmgs. Verify that `brew style --cask` on the file passes, and that the task 4.1 update regex matches it.

## 6. Go-live (Product Owner present; irreversible)

- [ ] 6.1 New repository with a clean single commit (design D9):
  - Build the inventory of personal values from the reference invoice PDF, the working tree, `openspec/` and the old history (real sender and client names, streets, postal/city lines, IBAN grouped and ungrouped, and the same in commit messages).
  - Write the replace-text mapping file **outside** the repo, mapping each value to a made-up one. The IBAN maps to a checksum-valid example such as `DE89 3704 0044 0532 0130 00`. The PO reviews the mapping.
  - Create a sibling directory with `git init -b main`. Copy the old `main`'s tracked files plus the whole `openspec/` directory, without `.git`, `.claude/`, `dist/`, `target/`, `.cargo/config.toml`, `output-test` or the client PDF. Apply the mapping to every text file. Remove the `openspec/*` and `!openspec/config.yaml` lines from `.gitignore`.
  - Make one initial commit. The old repository is not modified.

  Verify in the new directory:
  - `git log` shows exactly one commit, and `git ls-files` shows no PDF and nothing under `.claude/`, `dist/` or `.cargo/`;
  - a case-insensitive search of every tracked file and of the commit message finds none of the inventoried values;
  - `cargo test --locked` passes, with no `.cargo/config.toml` in place;
  - `git status` is clean and `openspec/` is tracked.
- [ ] 6.2 Publish: create the public GitHub repos `pravinbashyal/billing` and `pravinbashyal/homebrew-billy-jean`, and push `main` to each. Create a fine-grained PAT with write access to Contents on the tap only, and store it as the `HOMEBREW_TAP_TOKEN` Actions secret on `pravinbashyal/billing`. Verify with `gh repo view` that both repos are public and have `main`, and with `gh secret list -R pravinbashyal/billing` that the secret is listed.
- [ ] 6.3 First release end to end: run `scripts/release.sh --version 1.0.0`, push `release/v1.0.0`, open and merge the PR. Verify:
  - tag `v1.0.0` exists on the merged commit;
  - the `release.yml` run succeeds;
  - GitHub Release `v1.0.0` has `Billy-Jean-1.0.0-arm64.dmg` and `Billy-Jean-1.0.0-x86_64.dmg` with the unsigned-build notes;
  - the tap has a bot commit whose hashes equal `shasum -a 256` of the downloaded assets;
  - on this Mac, `brew tap pravinbashyal/billy-jean && brew install --cask billy-jean` prints the caveat and installs `/Applications/Billy Jean.app`, which launches with visible text after the one-time Gatekeeper override and finds the existing identity;
  - `brew uninstall --cask billy-jean` removes the app.

## Workflow follow-up

- Archive this change after 6.3. Because `openspec/` is untracked from now on, the archive lives only locally.
- Optional later changes: an app icon and window title, notarization if an Apple Developer account is obtained, and a `ProjectDirs` rename with identity migration if the config-folder mismatch ever matters.
