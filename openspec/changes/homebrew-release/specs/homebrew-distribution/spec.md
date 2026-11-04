## Purpose

Defines installing, upgrading and removing Billy Jean through the personal Homebrew tap `pravinbashyal/homebrew-billy-jean`, and keeping that tap's Cask in step with each published release automatically.

## ADDED Requirements

### Requirement: Tap installs the app for the host architecture
The tap `pravinbashyal/billy-jean` SHALL provide a Cask `billy-jean`. On macOS, installing it SHALL download the current release's disk image for the host architecture (arm64 or Intel), verify its SHA-256, and install `Billy Jean.app` into the Applications folder.

#### Scenario: Install on Apple silicon
- **WHEN** a user on an arm64 Mac runs `brew tap pravinbashyal/billy-jean` then `brew install --cask billy-jean`
- **THEN** the arm64 `.dmg` of the current release is downloaded, its checksum matches, and `/Applications/Billy Jean.app` exists

#### Scenario: Install on Intel
- **WHEN** a user on an x86_64 Mac runs the same commands
- **THEN** the x86_64 `.dmg` is downloaded instead and `/Applications/Billy Jean.app` exists

#### Scenario: Checksum mismatch is rejected
- **WHEN** the downloaded image's SHA-256 differs from the one in the Cask
- **THEN** Homebrew aborts the install and no app is installed

### Requirement: Installed app launches after a one-time override
After a Cask install, `Billy Jean.app` SHALL open its editor window once the user has allowed it through Gatekeeper a single time. The app SHALL NOT be reported as damaged because of an invalid or broken signature.

#### Scenario: First launch after override
- **WHEN** a user installs via the tap, gets past Gatekeeper once as the caveat describes, and opens Billy Jean
- **THEN** the invoice editor window appears with visible text

### Requirement: Cask discloses the unsigned, unofficial build
The Cask SHALL declare a macOS-only requirement. It SHALL show a caveat saying that it comes from a personal, unofficial tap and installs an unsigned, non-notarized build.

#### Scenario: Caveat shown on install
- **WHEN** a user runs `brew install --cask billy-jean`
- **THEN** Homebrew prints the unsigned, unofficial-build caveat

### Requirement: Cask is updated automatically on each release
Each successful release build SHALL update the Cask in the tap repository to the new version and the new per-architecture SHA-256 values, and push that change. If nothing changed, it SHALL skip the commit.

#### Scenario: Upgrade picks up a new release
- **WHEN** release `v1.1.0` is published while a user has `1.0.0` installed, and the user runs `brew update` then `brew upgrade --cask billy-jean`
- **THEN** `Billy Jean.app` is replaced by `1.1.0` without anyone editing the Cask by hand

#### Scenario: Cask values match published assets
- **WHEN** the tap is updated for a release
- **THEN** the Cask's version equals the release version, and each architecture's SHA-256 equals that of the matching `.dmg` attached to the GitHub Release

### Requirement: App can be cleanly uninstalled
`brew uninstall --cask billy-jean` SHALL remove `Billy Jean.app` from the Applications folder, and `brew untap pravinbashyal/billy-jean` SHALL remove the tap.

#### Scenario: Uninstall removes the app
- **WHEN** a user runs `brew uninstall --cask billy-jean`
- **THEN** `/Applications/Billy Jean.app` no longer exists
