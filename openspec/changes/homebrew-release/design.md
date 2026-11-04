# Design

## Context

See proposal.md (Why) for the motivation. The facts below shaped the approach:

- **Reference project.** `pravinbashyal/pdf-tools` (Electron) ships as a Cask from the personal tap `pravinbashyal/homebrew-pdf-tools`:
  - `release.yml` checks the tag against the version, builds unsigned arm64 and x64 `.dmg`s, verifies the signatures, and publishes them with `softprops/action-gh-release@v2`.
  - It then updates `Casks/<name>.rb` in the tap with a Python regex (the `version` line plus the `sha256` inside the `on_arm`/`on_intel` blocks), pushing with the `HOMEBREW_TAP_TOKEN` secret.
  - `tag-release-pr.yml` tags `main` when a `release/*` PR merges and explicitly starts `release.yml` (`workflow_dispatch` with a `tag` input), because a tag pushed with `GITHUB_TOKEN` does not trigger workflows.
  - pdf-tools learned that a bundle modified after it is signed carries a *broken* signature ("damaged"), so it re-signs ad-hoc as the last step (`scripts/afterSign.js`).
- **This app.** It is a gpui (`gpui-pre =0.3.11`) GUI app.
  - The Noto Sans font is embedded with `include_bytes!` in `src/main.rs` and `src/pdf.rs`, so nothing is loaded at runtime from `assets/`.
  - `runtime_shaders` means building needs no Metal shader compiler.
  - Per-user identity lives at `ProjectDirs("com", "pravinbashyal", "billing")`, which is `~/Library/Application Support/com.pravinbashyal.billing/identity.json` (`src/identity_store.rs`).
- **Blockers in the current repo.** `Cargo.toml` has a `[patch.crates-io]` block and an optional `gpui-agent` dependency, all pointing at `/Users/pravinbashyal/project/gpui-pre-0.3.11-agent/...`, which is not a git repo. Cargo needs path dependencies to exist even when their feature is off, so no CI runner can build the current manifest.
  - Commit `2327209` tracks a real client invoice PDF.
  - Test fixtures in `src/app_state.rs` (and siblings) use real sender and client names, addresses and an IBAN.
  - There is no remote, LICENSE, README or `.github/`.
- **Product Owner decisions** (exploration, 2026-11-03):
  - Rewrite the history and publish the repo publicly.
  - Move the dev patch to an untracked `.cargo/config.toml` and remove `gpui-agent`.
  - Ship two dmgs (arm64 and x86_64).
  - Use a release-PR flow now, with a Rust tool.
  - Name the product "Billy Jean".
  - Apply this change only after `add-invoice-builder` merges to `main`.

## Goals / Non-Goals

**Goals:**
- Reuse pdf-tools' release and tap mechanics nearly unchanged, so both projects work the same way and its Cask-update script carries over.
- Change no runtime behaviour. The app the user sees is identical apart from its bundle name.
- Keep personal data out of every published commit, not just the latest one.

**Non-Goals:**
- Apple Developer ID signing and notarization, and inclusion in the official `homebrew/cask`.
- Universal (`lipo`) binaries, Linux or Windows builds, auto-update inside the app.
- An app icon or a window title. The menu-bar name comes from `CFBundleName`.
- Publishing the crate to crates.io.

## Decisions

### D1. Cask with a `.app` in a `.dmg`, not a Formula
Billy Jean is a windowed GUI app. A Formula would install a bare binary into `bin/`, with no Dock icon, Spotlight entry or app menu name. A Cask installing `Billy Jean.app` into `/Applications` matches pdf-tools and macOS conventions. A `.dmg` (rather than `.zip`) also matches pdf-tools, and a direct downloader gets a familiar drag-to-Applications image.

### D2. Hand-rolled bundling script over `cargo-bundle`
`scripts/bundle-macos.sh <target-triple> <version>`:
1. Runs `cargo build --release --locked --target <triple>`.
2. Assembles `Billy Jean.app/Contents/{MacOS/billing, Info.plist, Resources/OFL.txt}`, filling `CFBundleShortVersionString` and `CFBundleVersion` from the version into `packaging/Info.plist`.
3. Runs `codesign --force --deep --sign -` as the **last** change to the bundle.
4. Runs `hdiutil create -format UDZO` on a staging folder holding the app plus an `/Applications` symlink, producing `dist/Billy-Jean-<version>-<arch>.dmg`. `<arch>` is `arm64` or `x86_64`.

Info.plist keys:
- `CFBundleName` and `CFBundleDisplayName` = "Billy Jean"
- `CFBundleIdentifier` = `com.pravinbashyal.billy-jean`
- `CFBundleExecutable` = `billing`
- `CFBundlePackageType` = `APPL`
- `NSHighResolutionCapable` = true
- `LSMinimumSystemVersion` = `11.0`
- `LSApplicationCategoryType` = `public.app-category.business`

Alternatives: `cargo-bundle` is barely maintained, and it controls neither signing order nor dmg creation, so we would still need a script around it. `cargo-packager` is heavier than a 3-file bundle needs. The script also runs locally, so CI and developers build the same thing.

The asset name has no spaces (`Billy-Jean-...`). GitHub rewrites spaces in asset names to dots, which is why pdf-tools ended up with `PDF.Tools-...`. Avoiding that keeps the Cask URL predictable.

### D3. Both architectures cross-compiled on one arm64 runner
`release.yml` runs on `macos-latest` (arm64), installs the `aarch64-apple-darwin` and `x86_64-apple-darwin` Rust targets, and calls the bundling script once per target, with `MACOSX_DEPLOYMENT_TARGET=11.0`. Apple's SDK cross-links both architectures natively, and nothing in this crate runs a build script that needs to execute target binaries.

Alternative: a separate Intel runner job (`macos-15-intel`) adds a matrix and an artifact hand-off for no gain. It is kept only as the fallback if cross-compiling fails (see Risks). This matches PO decision Q3(a), two dmgs. Universal binaries were rejected.

### D4. Naming: "Billy Jean" outside, `billing` inside
| Item | Value | Changed? |
|---|---|---|
| App bundle / display / menu name | `Billy Jean.app`, "Billy Jean" | new |
| Bundle ID | `com.pravinbashyal.billy-jean` | new (there was no bundle before) |
| Cask token / tap | `billy-jean` / `pravinbashyal/homebrew-billy-jean` | new |
| GitHub source repo | `pravinbashyal/billing` | new |
| Crate, lib and binary (`CFBundleExecutable`) | `billing` | unchanged |
| Config dir (`ProjectDirs`) | `~/Library/Application Support/com.pravinbashyal.billing` | unchanged |

Rationale:
- Renaming the crate would rewrite every `use billing::...`, doc link and test temp-dir prefix without any user-visible benefit. The executable name only shows up inside the bundle and in `ps`.
- Keeping `ProjectDirs` as-is means **no migration**: the identity the PO has already saved (sender and bank) is still found by the bundled app.
- The bundle ID never existed before, so nothing keyed by bundle ID needs migrating. That covers `NSUserDefaults`, saved window state and Gatekeeper approvals.
- The cost is that the bundle ID (`...billy-jean`) and the config folder (`...billing`) don't match. That is cosmetic, and documented here.

Rejected alternative: switching `ProjectDirs` to `"Billy Jean"`, which gives `com.pravinbashyal.Billy-Jean`. That needs one-time migration code that moves `identity.json` and handles both locations existing, which is real behaviour risk for a cosmetic gain. If it's wanted later, it can be its own change.

### D5. Dev-only patch moves to an untracked `.cargo/config.toml`, and `gpui-agent` is removed
- `[patch.crates-io]` moves verbatim to `.cargo/config.toml`, which Cargo supports. That file is gitignored.
- The `gpui-agent` optional path dependency, the `gpui-agent` feature, and the `#[cfg(feature = "gpui-agent")] gpui_agent::init(cx);` call in `src/main.rs` are deleted.
- `gpui-mcp` comes from crates.io and stays as the dev inspection path.
- `gpui-driver` is an empty Cargo feature (`gpui-driver = []`), kept by PO decision as part of this dev-tooling choice. It gates three calls in `src/editor.rs` that exist only in the patched `gpui-pre` fork: `window.driver_enabled()`, `window.set_driver_enabled(true)` and `window.driver_insert_text(...)`.
  - Without the feature, `gpui-mcp` builds on stock crates.io `gpui-pre =0.3.11`, and the TypeText action returns the error "typing needs the gpui-driver feature (patched gpui-pre)".
  - With the feature and the untracked `.cargo/config.toml` patch, typing works.
  - Release and Homebrew builds use neither feature.
- The committed `Cargo.lock` is resolved **without** the patch, from crates.io sources only.

**Lockfile hazard.** Patched crates show up in `Cargo.lock` with no `source`/`checksum`. While the local `.cargo/config.toml` exists, any local `cargo` command rewrites the lock to that form. Committing it would break `--locked` in CI. Guards:
- `release.sh` refuses to run if `Cargo.lock` has a `gpui-pre*` entry without a `source = "registry+..."` line.
- CI always builds with `--locked`, so a polluted lock fails loudly instead of shipping.
- The README's developer section tells developers not to commit lockfile changes made with the patch active.

With `gpui-agent` gone, the `gpui-driver` feature is the patch's only consumer. The PO keeps both; see Open Questions.

Alternatives (PO-rejected): publishing the patched crates as git dependencies, or having CI strip the manifest.

### D6. Versioning: `git-cliff` plus a small `scripts/release.sh`, not `cargo-release`
The pdf-tools flow needs a tool that (1) computes the semver bump from Conventional Commits, (2) writes `CHANGELOG.md`, and (3) does **not** tag, because tagging happens in CI on merge.
- `git-cliff` does (1) with `git cliff --bumped-version` and (2) with `git cliff --bump --unreleased --prepend CHANGELOG.md`. It never tags or pushes.
- `cargo-release` does none of (1) or (2). It needs an explicit bump level, generates no changelog (it only does pre-release text replacements), and by default tags, pushes and publishes to crates.io, all of which would have to be turned off. That fights the flow instead of fitting it.

`scripts/release.sh [--version X.Y.Z]`:
1. Requires a clean tree on an up-to-date `main` and the lockfile guard from D5 to pass.
2. Computes the version: either the explicit one, used for the first `1.0.0`, or `git cliff --bumped-version`.
3. Creates `release/vX.Y.Z`.
4. Rewrites the `[package]` `version` in `Cargo.toml` and runs `cargo update --workspace --offline` so only the root package's lock entry changes.
5. Prepends the changelog and makes one commit, `chore(release): vX.Y.Z`. Pushing the branch and opening the PR (`gh pr create`) is left to the user, as in pdf-tools.

`cliff.toml` uses the Conventional Commits preset with `filter_unconventional = true`, so the pre-1.0 commits (`add-invoice-builder: task ...`) don't clutter the changelog. Commit convention from 1.0.0 on: `feat:`, `fix:`, `chore:`, `docs:`, `ci:`, `refactor:`, `test:`, with `!` or `BREAKING CHANGE:` for a major bump. This is documented in the README and not enforced by a hook.

### D7. Workflows mirror pdf-tools
- **`tag-release-pr.yml`** (ubuntu-latest): on a `pull_request` `closed` event, when the PR merged from a `release/*` branch into `main`:
  - reads the version with `cargo metadata --no-deps --format-version 1` and `jq`;
  - refuses if the tag already exists;
  - tags and pushes;
  - runs `gh workflow run release.yml -f tag=vX.Y.Z`.

  Permissions: `contents: write`, `actions: write`.
- **`release.yml`** (macos-latest): triggered by `push` of `v*` tags or by `workflow_dispatch` with a `tag` input. Steps:
  1. resolve the tag and check out that ref;
  2. set up `dtolnay/rust-toolchain@stable` with both targets, plus `Swatinem/rust-cache`;
  3. version check (`cargo metadata` against the tag);
  4. run `bundle-macos.sh` for both triples;
  5. run `codesign --verify --deep --strict` on both apps;
  6. publish with `softprops/action-gh-release@v2`, `fail_on_unmatched_files: true`, and pdf-tools' notes reworded for Billy Jean;
  7. check out `pravinbashyal/homebrew-billy-jean` with `HOMEBREW_TAP_TOKEN`;
  8. run the pdf-tools Python regex update on `Casks/billy-jean.rb`;
  9. commit if anything changed and push to `main`.

### D8. Cask shape
Copied from `Casks/pdf-tools.rb`:
- `version`, then `on_arm`/`on_intel` blocks, each with `sha256` followed by a `url "https://github.com/pravinbashyal/billing/releases/download/v#{version}/Billy-Jean-#{version}-<arch>.dmg"`. The `sha256`-then-`url` order inside each block is what the update regex relies on.
- `name "Billy Jean"`, `desc`, `homepage`.
- `depends_on :macos` (explicit floor is flagged redundant by `brew style`; `LSMinimumSystemVersion` 11.0 enforces it), matching `LSMinimumSystemVersion` 11.0.
- `app "Billy Jean.app"`, the same unsigned-build caveat, and `zap trash: "~/Library/Application Support/com.pravinbashyal.billing"`.

`zap` deletes data only on an explicit `brew uninstall --zap`. A plain `brew uninstall` keeps the identity.

### D9. Publish from a brand-new repository, not a rewritten history
PO decision: instead of rewriting the old history with `git filter-repo`, create a completely new repository and keep only the code and `openspec/`. The old repository stays untouched on this machine as a private backup, and its history is never published. Run after this change's code tasks (groups 1-5) are committed on `main`, and before the first push:
1. Build the inventory of personal values (reference invoice PDF plus a search of the working tree and `openspec/`). Write the `--replace-text`-style mapping file **outside** the repo, because it contains the real values. Map each real name, street, postal-city line and the IBAN to a made-up one.
2. Create the new directory as a sibling of the old repo and `git init -b main` there. Copy in the tracked files of the old `main` plus the whole `openspec/` directory (including this change). Leave out `.git`, `.claude/`, `dist/`, `target/`, `.cargo/config.toml`, `output-test` and the client PDF.
3. Apply the mapping to every text file in the copy, in file order (literal rules and `regex:` rules). Skip binary files such as the fonts.
4. Replace the `.gitignore` entries `openspec/*` and `!openspec/config.yaml` with nothing, so `openspec/` is tracked. Keep `.claude/`, `dist/`, `.cargo/config.toml`, `output-test`, `/target`.
5. Verify, then make one initial commit (`feat: initial public release of Billy Jean` or similar, with the attribution trailer). The commit author is the owner's existing public identity.

The fake IBAN must be a checksum-valid German example IBAN (for example `DE89 3704 0044 0532 0130 00`), so the IBAN-validation tests still pass.

Trade-off: the per-task commit history the five-role workflow produced is not published. The PO accepts this. The old repository keeps it locally.

## Risks / Trade-offs

- **Unpatched build.** [Risk] The app may not build or render against stock `gpui-pre =0.3.11`. → The first task builds and launches from a fresh clone. If it fails, stop and go back to the PO: their Q2(b), publishing the patched crates as git dependencies, would change this design.
- **Cross-compiling for Intel.** [Risk] The x86_64 build on an arm64 runner fails, for example if a dependency's build script misbehaves. → Fall back to a second job on `macos-15-intel` and hand the dmg over with `upload-artifact`. Nothing changes in the specs or the Cask.
- **Unsigned on Apple silicon.** [Risk] Ad-hoc-signed, quarantined apps need the "Open Anyway" override on macOS 15+, and Homebrew no longer offers `--no-quarantine`. → This is documented in the README, the release notes and the Cask caveat, the same as pdf-tools. A broken signature (the actual "damaged" failure) is prevented by signing last and verifying strictly in CI.
- **Anonymization misses a value.** [Risk] The mapping misses a personal value, for example the client's address or a sender detail in a spec under `openspec/`, which is now published. → The inventory is built from the reference PDF and a search of the working tree *and* the old history before copying. A post-copy search of every file and of the commit message for every inventoried value gates the push. A public repo with a fresh history has no old commits that could leak.
- **Polluted lockfile.** [Risk] A lockfile rewritten by the local dev patch gets committed. → The D5 guards apply: the `release.sh` check and `--locked` in CI.
- **Unpinned toolchain.** [Risk] `stable` on CI drifts from the local `rustc 1.99.1`. → Edition 2024 only needs 1.85 or later. If drift bites, add a `rust-toolchain.toml`.
- **Token scope.** [Risk] The `HOMEBREW_TAP_TOKEN` PAT expires and the tap stops updating while releases still publish. → The release job fails visibly at the tap step. The README notes when to rotate the token.

## Migration Plan

1. Apply the code tasks (manifest, docs, packaging, versioning, workflows, tap files) on a branch off `main` after `add-invoice-builder` merges, then merge.
2. Build the new repository with a single initial commit (D9) and verify.
3. Create the public `pravinbashyal/billing` and `pravinbashyal/homebrew-billy-jean` repos, push, and set `HOMEBREW_TAP_TOKEN`.
4. Run `scripts/release.sh --version 1.0.0`, open and merge the release PR, and watch the tag, the release and the tap update. Then install with `brew` on this Mac and launch.

Rollback: before step 3, delete the new directory and redo step 2; the old repository is untouched. After publishing, a bad release is fixed forward with a patch release. The tap can be reverted to the previous Cask commit.

## Open Questions

- Resolved by the Product Owner: keep the untracked dev patch in `.cargo/config.toml` and keep the `gpui-driver` Cargo feature. `gpui-driver` gates the three patched-fork calls in `src/editor.rs` (see D5), so the patch has a consumer. The D5 lockfile guards still apply. This choice doesn't affect the specs or tasks.
