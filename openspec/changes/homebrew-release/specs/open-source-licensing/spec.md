## Purpose

Defines what makes the Billy Jean source repository safe and usable to publish: licensing, accurate docs, a history free of client and personal data, and a manifest anyone can build from a fresh clone.

## ADDED Requirements

### Requirement: Repository declares a single open-source licence
The repository SHALL contain an MIT `LICENSE` file at its root, and the crate manifest SHALL declare the same licence. The bundled Noto Sans font SHALL keep its SIL Open Font License text in the repository.

#### Scenario: Licence file and manifest agree
- **WHEN** a reader opens `LICENSE` and the `license` field of `Cargo.toml`
- **THEN** both identify the MIT licence

#### Scenario: Font licence is retained
- **WHEN** a reader looks under `assets/fonts/`
- **THEN** `OFL.txt` is present alongside the bundled font

### Requirement: README documents build and install accurately
The repository SHALL contain a `README.md` that names the app "Billy Jean". It SHALL explain how to build and run from source, how to install through the Homebrew tap, and how to get past the Gatekeeper prompt for the unsigned build. Each command it lists SHALL work as written on macOS.

#### Scenario: Build from source as documented
- **WHEN** a user on macOS with a Rust toolchain follows the README's build-from-source steps in a fresh clone
- **THEN** the app builds and launches

#### Scenario: Homebrew install documented
- **WHEN** a user reads the README's install section
- **THEN** it gives `brew tap pravinbashyal/billy-jean` and `brew install --cask billy-jean`, plus the upgrade and uninstall commands, and describes the first-launch Gatekeeper override

### Requirement: Public history contains no client or personal data
No commit reachable from any published branch or tag SHALL contain a real client invoice document. Nor SHALL it contain the real client or sender names, street addresses or IBAN used as reference data during development. Test fixtures SHALL use made-up values instead.

#### Scenario: Client invoice absent from history
- **WHEN** the full published history is searched for PDF files at the repository root
- **THEN** the reference client invoice is not found in any commit

#### Scenario: Personal values absent from history
- **WHEN** the full published history is searched for each real name, address and IBAN on the inventory of personal values
- **THEN** no commit contains any of them

#### Scenario: Tests still pass with made-up fixtures
- **WHEN** `cargo test` runs on the published `main`
- **THEN** all tests pass

### Requirement: Published repository starts from a fresh history
The published repository SHALL be a new repository with a single initial commit, created from the current code and `openspec/`. The old history SHALL NOT be published. The initial commit SHALL NOT contain `.claude/`, `dist/`, `.cargo/config.toml` or the reference client invoice. The repository SHALL ignore `.claude/`, `dist/` and `.cargo/config.toml` so they stay local.

#### Scenario: Single clean commit
- **WHEN** the published history is listed
- **THEN** it consists of one initial commit, and no path under `.claude/`, `dist/` or `.cargo/` appears in it

#### Scenario: New local-only files stay untracked
- **WHEN** a developer creates a file under `.claude/` or `.cargo/config.toml` and runs `git status`
- **THEN** the file is not offered as an untracked file to commit

### Requirement: Manifest builds from a fresh clone
The committed `Cargo.toml` and `Cargo.lock` SHALL resolve and build on a machine that has only the public repository and crates.io. They SHALL NOT reference any absolute or machine-local filesystem path.

#### Scenario: Clean-machine build
- **WHEN** a fresh clone is built with `cargo build --release --locked` on a machine without `/Users/pravinbashyal/project/gpui-pre-0.3.11-agent`
- **THEN** the build succeeds

#### Scenario: No local paths in the manifest
- **WHEN** the committed `Cargo.toml` is inspected
- **THEN** it has no `path =` dependency and no `[patch]` section pointing at a local directory
