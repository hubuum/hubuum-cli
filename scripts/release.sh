#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/Cargo.toml"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/release.sh prepare <version>
  ./scripts/release.sh tag

Commands:
  prepare <version>  Create release/v<version> from main, bump Cargo.toml, roll
                     Unreleased notes into the release section, and run release
                     checks.
  tag                Verify main is clean and create annotated tag
                     v<current-version>.
EOF
}

die() {
  echo "$*" >&2
  exit 1
}

ensure_clean_worktree() {
  if ! git -C "${REPO_ROOT}" diff --quiet --ignore-submodules HEAD --; then
    die "Working tree has uncommitted changes. Commit or stash them before running the release helper."
  fi

  if [[ -n "$(git -C "${REPO_ROOT}" ls-files --others --exclude-standard)" ]]; then
    die "Working tree has untracked files. Commit, move, or clean them before running the release helper."
  fi
}

current_branch() {
  git -C "${REPO_ROOT}" rev-parse --abbrev-ref HEAD
}

require_branch() {
  local expected="$1"
  local actual

  actual="$(current_branch)"
  if [[ "${actual}" != "${expected}" ]]; then
    die "Expected to be on branch ${expected}, but found ${actual}."
  fi
}

manifest_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "${CARGO_TOML}" | head -n 1
}

normalized_version() {
  printf '%s\n' "${1#v}"
}

prepare_release() {
  local version="$1"
  local normalized
  local branch_name
  local tag_name

  normalized="$(normalized_version "${version}")"
  branch_name="release/v${normalized}"
  tag_name="v${normalized}"

  ensure_clean_worktree
  require_branch "main"

  if git -C "${REPO_ROOT}" show-ref --verify --quiet "refs/heads/${branch_name}"; then
    die "Branch ${branch_name} already exists."
  fi

  if git -C "${REPO_ROOT}" show-ref --verify --quiet "refs/tags/${tag_name}"; then
    die "Tag ${tag_name} already exists."
  fi

  "${REPO_ROOT}/scripts/prepare-release.sh" prepare "${normalized}" --base HEAD
  cargo check --locked
  (
    cd "${REPO_ROOT}/cli_command_derive"
    cargo check --locked
  )
  "${REPO_ROOT}/scripts/check-version-bump.sh"

  cat <<EOF
Release branch created: ${branch_name}

Next steps:
1. Review and edit CHANGELOG.md if the release notes need cleanup.
2. Commit the release branch changes.
3. Open and merge the release branch.
4. After merge, check out main and run ./scripts/release.sh tag
EOF
}

tag_release() {
  local version
  local tag_name

  ensure_clean_worktree
  require_branch "main"

  version="$(manifest_version)"
  [[ -n "${version}" ]] || die "Could not determine package version from Cargo.toml"
  tag_name="v${version}"

  if git -C "${REPO_ROOT}" show-ref --verify --quiet "refs/tags/${tag_name}"; then
    die "Tag ${tag_name} already exists."
  fi

  "${REPO_ROOT}/scripts/check-release-readiness.sh" "${tag_name}"
  cargo check --locked
  (
    cd "${REPO_ROOT}/cli_command_derive"
    cargo check --locked
  )
  git -C "${REPO_ROOT}" tag -a "${tag_name}" -m "Release ${tag_name}"

  cat <<EOF
Created annotated tag ${tag_name}

Next steps:
1. Push main and the tag: git push origin main "${tag_name}"
2. Wait for GitHub Actions to publish the release artifacts.
EOF
}

main() {
  if [[ $# -lt 1 ]]; then
    usage
    exit 1
  fi

  cd "${REPO_ROOT}"

  case "$1" in
    prepare)
      [[ $# -eq 2 ]] || die "prepare requires exactly one version argument."
      prepare_release "$2"
      ;;
    tag)
      [[ $# -eq 1 ]] || die "tag does not take any arguments."
      tag_release
      ;;
    -h|--help|help)
      usage
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
