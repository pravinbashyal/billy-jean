#!/usr/bin/env bash
# Starts a release. Computes the next version from the Conventional Commits since the
# last v* tag (or takes --version), sets it in Cargo.toml and Cargo.lock, prepends the
# new section to CHANGELOG.md, and commits the three files as one commit on a
# release/vX.Y.Z branch. It never tags and never pushes. Pushing the branch and opening
# the release PR is left to you; merging that PR tags main and starts the build in CI
# (.github/workflows/tag-release-pr.yml).
#
# Usage: scripts/release.sh [--version X.Y.Z]
#   --version  use this version instead of computing one. Use it for the first 1.0.0.
#
# Refuses to run unless main is clean and equal to origin/main, and refuses a
# Cargo.lock with a gpui-pre* entry that has no crates.io source (design D5).
set -euo pipefail

usage() {
  echo "usage: $0 [--version X.Y.Z]" >&2
  exit 2
}

die() {
  echo "error: $*" >&2
  exit 1
}

explicit_version=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || usage
      explicit_version="$2"
      shift 2
      ;;
    -h | --help) usage ;;
    *) echo "error: unknown argument '$1'" >&2; usage ;;
  esac
done

semver_re='^[0-9]+\.[0-9]+\.[0-9]+$'
if [[ -n "$explicit_version" && ! "$explicit_version" =~ $semver_re ]]; then
  echo "error: --version '$explicit_version' is not X.Y.Z" >&2
  usage
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

command -v git >/dev/null || die "git is not installed"
command -v git-cliff >/dev/null || die "git-cliff is not installed (brew install git-cliff, or cargo install git-cliff --locked)"
command -v cargo >/dev/null || die "cargo is not installed"

# Lockfile guard (design D5). A gpui-pre* entry without a registry source means the
# local .cargo/config.toml patch rewrote Cargo.lock. Committing that breaks --locked.
check_lock() {
  local bad
  bad="$(awk '
    function flush() { if (name ~ /^gpui-pre/ && !has_source) bad = bad " " name }
    /^\[\[package\]\]/ { flush(); name = ""; has_source = 0 }
    /^name = "/ { name = $3; gsub(/"/, "", name) }
    /^source = "registry\+/ { has_source = 1 }
    END { flush(); print bad }
  ' "$root/Cargo.lock")"
  if [[ -n "${bad// /}" ]]; then
    echo "error: Cargo.lock has gpui-pre entries without a crates.io source:$bad" >&2
    echo "       this happens when Cargo.lock is rewritten while the local .cargo/config.toml" >&2
    echo "       patch is active. Move the patch aside, regenerate Cargo.lock, and retry." >&2
    exit 1
  fi
}

# 1. Clean tree, on main, equal to origin/main.
branch="$(git symbolic-ref --quiet --short HEAD || true)"
[[ "$branch" == "main" ]] || die "must run on main (currently on '${branch:-a detached HEAD}')"
[[ -z "$(git status --porcelain)" ]] || die "working tree is not clean; commit or stash first"
git remote get-url origin >/dev/null 2>&1 || die "no 'origin' remote to check main against"
git fetch --quiet origin main || die "could not fetch origin main"
git rev-parse --verify --quiet origin/main >/dev/null || die "origin has no main branch"
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
  || die "main is not up to date with origin/main; pull or push first"

# 2. Lockfile guard, before anything is changed.
[[ -f Cargo.lock ]] || die "Cargo.lock is missing"
check_lock

# 3. Version.
if [[ -n "$explicit_version" ]]; then
  version="$explicit_version"
else
  [[ -n "$(git tag --list 'v[0-9]*')" ]] \
    || die "no v* tag exists yet; pass --version X.Y.Z for the first release"
  bumped="$(git cliff --bumped-version)" || die "git cliff --bumped-version failed"
  version="${bumped#v}"
fi
[[ "$version" =~ $semver_re ]] || die "computed version '$version' is not X.Y.Z"
tag="v$version"
release_branch="release/$tag"

git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null && die "tag $tag already exists locally"
git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1 && die "tag $tag already exists on origin"
git rev-parse --quiet --verify "refs/heads/$release_branch" >/dev/null && die "branch $release_branch already exists locally"
git ls-remote --exit-code --heads origin "$release_branch" >/dev/null 2>&1 && die "branch $release_branch already exists on origin"

# From here on, any exit before the commit rolls the tree back to main (see on_exit).
scratch_dir=""
release_started=0
changelog_tracked=0
git ls-files --error-unmatch CHANGELOG.md >/dev/null 2>&1 && changelog_tracked=1

rollback() {
  echo "error: release $tag failed; restoring the tree and returning to main" >&2
  git reset -q -- Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true
  git checkout -q -- Cargo.toml Cargo.lock 2>/dev/null || true
  if [[ "$changelog_tracked" -eq 1 ]]; then
    git checkout -q -- CHANGELOG.md 2>/dev/null || true
  else
    rm -f CHANGELOG.md
  fi
  git checkout -q main 2>/dev/null || true
  git branch -q -D "$release_branch" 2>/dev/null || true
}

on_exit() {
  local status=$?
  [[ -n "$scratch_dir" ]] && rm -rf "$scratch_dir"
  if [[ "$status" -ne 0 && "$release_started" -eq 1 ]]; then
    rollback
  fi
}
trap on_exit EXIT

# 4. Branch and version.
git checkout -q -b "$release_branch"
release_started=1
echo "==> setting [package] version to $version"
perl -i -pe '
  if (/^\[/) { $in_package = /^\[package\]\s*$/ ? 1 : 0 }
  if ($in_package && !$done && /^version = /) { s/^version = .*/version = "'"$version"'"/; $done = 1 }
' Cargo.toml
changed_lines="$(git diff --numstat -- Cargo.toml | cut -f1)"
[[ -z "$changed_lines" || "$changed_lines" == "1" ]] || die "Cargo.toml changed more than the version line"

# Run cargo from a neutral directory: cargo reads .cargo/config.toml from the current
# directory upwards, so this keeps the local dev patch from rewriting the lockfile.
echo "==> cargo update --workspace --offline"
scratch_dir="$(mktemp -d)"
(cd "$scratch_dir" && cargo update --manifest-path "$root/Cargo.toml" --workspace --offline)
check_lock

# 5. Changelog.
echo "==> prepending $tag to CHANGELOG.md"
[[ -f CHANGELOG.md ]] || : > CHANGELOG.md
git cliff --unreleased --tag "$tag" --prepend CHANGELOG.md

# 6. One commit, limited to the release files.
git add -- Cargo.toml Cargo.lock CHANGELOG.md
while IFS= read -r staged_file; do
  case "$staged_file" in
    Cargo.toml | Cargo.lock | CHANGELOG.md) ;;
    *) die "unexpected file staged for the release commit: $staged_file" ;;
  esac
done < <(git diff --cached --name-only)
git diff --cached --quiet && die "nothing to commit for $tag"
git commit -q -m "chore(release): $tag"
release_started=0

echo "==> committed chore(release): $tag on $release_branch (not tagged)"
echo "    next: git push -u origin $release_branch, then open a PR into main"
