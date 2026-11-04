## Purpose

Defines what a Billy Jean release is and how one is made: a reviewed version bump and changelog from Conventional Commits, a tag created on merge, and a version-checked build of unsigned per-architecture macOS disk images published to a GitHub Release.

## ADDED Requirements

### Requirement: Version bump and changelog are derived from commit history
A local release command SHALL compute the next semantic version from the Conventional Commits since the last `v*` tag. A `fix:` gives a patch bump, a `feat:` a minor bump, and `!` or a `BREAKING CHANGE:` footer a major bump. The command SHALL write that version to the crate manifest and lockfile and prepend the section to `CHANGELOG.md`, as one commit. It SHALL NOT create or push a tag.

#### Scenario: Feature commits produce a minor bump
- **WHEN** the latest tag is `v1.0.0`, the commits since then include a `feat:` commit and no breaking change, and the release command runs
- **THEN** `Cargo.toml` and `Cargo.lock` declare `1.1.0`, `CHANGELOG.md` gains a `1.1.0` section listing those commits, and one new commit holds those changes

#### Scenario: No tag is created locally
- **WHEN** the release command finishes
- **THEN** no new git tag exists locally or on the remote

### Requirement: Releases are reviewed as a release PR
The release commit SHALL be proposed from a `release/vX.Y.Z` branch as a pull request into `main`. Its diff SHALL contain only the version bump and changelog.

#### Scenario: Release PR diff is limited
- **WHEN** a release PR is opened from `release/v1.1.0`
- **THEN** its diff touches only `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`

### Requirement: Merging a release PR tags and triggers the build
When a pull request from a `release/*` branch merges into `main`, automation SHALL read the version from the merged manifest, create and push the tag `v<version>`, and start the release build for that tag. If that tag already exists, it SHALL fail without overwriting or re-pushing it.

#### Scenario: Merge creates the tag and starts the build
- **WHEN** a PR from `release/v1.1.0` that sets the version to `1.1.0` is merged into `main`
- **THEN** tag `v1.1.0` points at the merged commit and a release build for `v1.1.0` starts

#### Scenario: Existing tag is not overwritten
- **WHEN** a release PR merges and its version's tag already exists
- **THEN** the automation fails and the existing tag is unchanged

#### Scenario: Non-release PRs do not tag
- **WHEN** a PR from a branch not prefixed `release/` merges into `main`
- **THEN** no tag is created

### Requirement: Release build refuses a version mismatch
The release build SHALL fail before producing any artifact if the tag's version (without the leading `v`) differs from the version declared in the crate manifest at that tag.

#### Scenario: Mismatched tag fails
- **WHEN** a release build runs for tag `v1.2.0` and the manifest at that tag declares `1.1.0`
- **THEN** the build fails and no GitHub Release is published

### Requirement: Each release ships per-architecture unsigned disk images
For each tag, the release build SHALL produce two `.dmg` files, one for Apple silicon (arm64) and one for Intel (x86_64), named with the version and architecture. Each SHALL contain a `Billy Jean.app` with bundle ID `com.pravinbashyal.billy-jean` and its version set to the release version.

#### Scenario: Two architecture images are produced
- **WHEN** the release build for `v1.1.0` succeeds
- **THEN** it produces `Billy-Jean-1.1.0-arm64.dmg` and `Billy-Jean-1.1.0-x86_64.dmg`

#### Scenario: Image contains the expected app
- **WHEN** either image is mounted
- **THEN** it contains `Billy Jean.app`, whose executable targets that image's architecture and whose short version string is `1.1.0`

### Requirement: App bundles carry a valid ad-hoc signature
Each `Billy Jean.app` SHALL carry an ad-hoc code signature that seals its final contents. The release build SHALL fail if strict signature verification of either bundle fails. No Apple Developer identity or notarization is used.

#### Scenario: Signature verifies
- **WHEN** the release build runs strict deep signature verification on each bundle
- **THEN** verification passes, and if it fails the build fails before publishing

### Requirement: App bundle includes third-party font licence
Each `Billy Jean.app` SHALL include the SIL Open Font License text for the embedded Noto Sans font in its resources.

#### Scenario: OFL shipped in bundle
- **WHEN** a user inspects `Billy Jean.app/Contents/Resources/`
- **THEN** `OFL.txt` is present

### Requirement: Disk images are published to a GitHub Release
A successful release build SHALL publish both `.dmg` files to the GitHub Release for that tag. The release notes SHALL state that the build is not code-signed or notarized and SHALL explain the Gatekeeper override for a direct download: the Privacy & Security "Open Anyway" button on macOS 15 and later, Control-click then Open on older versions, and `xattr -dr com.apple.quarantine` as the fallback.

#### Scenario: Release has both assets and notes
- **WHEN** the release build for `v1.1.0` completes
- **THEN** the GitHub Release `v1.1.0` has both `.dmg` assets attached and its notes include the unsigned-build override instructions
