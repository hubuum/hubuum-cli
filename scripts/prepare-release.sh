#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="${ROOT_DIR}/Cargo.toml"
CHANGELOG="${ROOT_DIR}/CHANGELOG.md"
CLI_DERIVE_CARGO_TOML="${ROOT_DIR}/cli_command_derive/Cargo.toml"

die() {
  echo "Error: $*" >&2
  exit 1
}

info() {
  echo "==> $*"
}

usage() {
  cat <<'EOF'
Usage:
  scripts/prepare-release.sh prepare <version|vversion> [--base <git-ref>] [--branch <branch-name>] [--no-branch]
  scripts/prepare-release.sh check <version|vversion>
  scripts/prepare-release.sh notes <version|vversion>

Commands:
  prepare  Create or switch to a release branch, bump Cargo.toml, and roll the
           CHANGELOG.md Unreleased section into the requested version.
  check    Verify Cargo.toml and CHANGELOG.md are ready for the requested version.
  notes    Print the requested version section from CHANGELOG.md.

Rules:
  - Versions must use stable SemVer, with or without a leading v.
  - prepare expects CHANGELOG.md to contain a ## [Unreleased] section with content.
  - prepare creates release/v<version> by default unless --branch is provided.
EOF
}

require_file() {
  local file_path="$1"

  [[ -f "${file_path}" ]] || die "Missing required file: ${file_path}"
}

ensure_repo_files() {
  require_file "${CARGO_TOML}"
  require_file "${CHANGELOG}"
}

validate_version() {
  local version="$1"

  [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || die "Version must use semantic versioning (for example 0.2.0 or 1.0.0-rc.1)"
}

normalize_version() {
  local version="$1"

  printf '%s\n' "${version#v}"
}

require_clean_worktree() {
  local status

  status="$(git -C "${ROOT_DIR}" status --short)"
  [[ -z "${status}" ]] || die "Worktree must be clean before preparing a release branch"
}

manifest_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "${CARGO_TOML}" | head -n 1
}

manifest_version_from_file() {
  local manifest_path="$1"

  sed -n 's/^version = "\(.*\)"/\1/p' "${manifest_path}" | head -n 1
}

set_manifest_file_version() {
  local manifest_path="$1"
  local version="$2"
  local current_version

  current_version="$(manifest_version_from_file "${manifest_path}")"
  [[ -n "${current_version}" ]] || die "Unable to determine package version from Cargo.toml"

  if [[ "${current_version}" == "${version}" ]]; then
    info "${manifest_path#${ROOT_DIR}/} already uses version ${version}"
    return
  fi

  perl -0pi -e "s/^version = \"\Q${current_version}\E\"/version = \"${version}\"/m" "${manifest_path}"
  info "Updated ${manifest_path#${ROOT_DIR}/} version ${current_version} -> ${version}"
}

set_manifest_versions() {
  local version="$1"

  set_manifest_file_version "${CARGO_TOML}" "${version}"

  if [[ -f "${CLI_DERIVE_CARGO_TOML}" ]]; then
    set_manifest_file_version "${CLI_DERIVE_CARGO_TOML}" "${version}"
  fi
}

refresh_lockfiles() {
  cargo check >/dev/null

  if [[ -f "${CLI_DERIVE_CARGO_TOML}" ]]; then
    (
      cd "${ROOT_DIR}/cli_command_derive"
      cargo check >/dev/null
    )
  fi

  info "Refreshed lockfiles"
}

check_secondary_manifest_versions() {
  local version="$1"
  local derive_version

  if [[ -f "${CLI_DERIVE_CARGO_TOML}" ]]; then
    derive_version="$(manifest_version_from_file "${CLI_DERIVE_CARGO_TOML}")"
    [[ "${derive_version}" == "${version}" ]] || die "cli_command_derive/Cargo.toml version does not match ${version}"
  fi
}

unreleased_has_content() {
  awk '
    /^## \[Unreleased\]$/ { in_section = 1; next }
    /^## \[/ && in_section { exit found ? 0 : 1 }
    in_section && $0 !~ /^[[:space:]]*$/ { found = 1 }
    END {
      if (!in_section) {
        exit 2
      }
      exit found ? 0 : 1
    }
  ' "${CHANGELOG}"
}

version_heading() {
  local version="$1"
  local escaped_version

  escaped_version="${version//./\\.}"
  grep -E "^## \[${escaped_version}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" "${CHANGELOG}" | head -n 1
}

section_has_content() {
  local heading="$1"

  awk -v heading="${heading}" '
    $0 == heading { in_section = 1; next }
    /^## \[/ && in_section { exit found ? 0 : 1 }
    in_section && $0 !~ /^[[:space:]]*$/ { found = 1 }
    END {
      if (!in_section) {
        exit 2
      }
      exit found ? 0 : 1
    }
  ' "${CHANGELOG}"
}

prepare_changelog() {
  local version="$1"
  local release_date="$2"
  local temp_file

  if version_heading "${version}" >/dev/null; then
    info "CHANGELOG.md already contains version ${version}"
    return
  fi

  if ! grep -q '^## \[Unreleased\]$' "${CHANGELOG}"; then
    die "CHANGELOG.md must contain a ## [Unreleased] section"
  fi

  if ! unreleased_has_content; then
    die "CHANGELOG.md needs release notes under ## [Unreleased] before preparing ${version}"
  fi

  temp_file="$(mktemp)"
  awk -v version="${version}" -v release_date="${release_date}" '
    /^## \[Unreleased\]$/ && !done {
      print $0
      print ""
      print "## [" version "] - " release_date
      print ""
      done = 1
      next
    }
    { print }
  ' "${CHANGELOG}" > "${temp_file}"

  mv "${temp_file}" "${CHANGELOG}"
  info "Rolled CHANGELOG.md Unreleased notes into ${version}"
}

ensure_branch() {
  local version="$1"
  local base_ref="$2"
  local branch_name="$3"
  local current_branch

  current_branch="$(git -C "${ROOT_DIR}" branch --show-current)"

  if [[ "${current_branch}" == "${branch_name}" ]]; then
    info "Already on ${branch_name}"
    return
  fi

  git -C "${ROOT_DIR}" rev-parse --verify "${base_ref}^{commit}" >/dev/null 2>&1 || die "Unknown git ref: ${base_ref}"

  if git -C "${ROOT_DIR}" rev-parse --verify "${branch_name}^{commit}" >/dev/null 2>&1; then
    die "Branch ${branch_name} already exists"
  fi

  git -C "${ROOT_DIR}" switch -c "${branch_name}" "${base_ref}" >/dev/null
  info "Created ${branch_name} from ${base_ref} for ${version}"
}

extract_notes() {
  local version="$1"

  awk -v version="${version}" '
    $0 ~ "^## \\[" version "\\]" { capture = 1 }
    capture && /^## \[/ && $0 !~ "^## \\[" version "\\]" { exit }
    capture { print }
  ' "${CHANGELOG}"
}

check_release() {
  local version="$1"
  local heading

  version="$(normalize_version "${version}")"
  validate_version "${version}"
  ensure_repo_files

  heading="$(version_heading "${version}" || true)"
  [[ -n "${heading}" ]] || die "CHANGELOG.md is missing a dated entry for ${version}"

  [[ "$(manifest_version)" == "${version}" ]] || die "Cargo.toml version does not match ${version}"
  check_secondary_manifest_versions "${version}"

  if ! section_has_content "${heading}"; then
    die "CHANGELOG.md entry for ${version} is empty"
  fi

  info "Release metadata for ${version} looks good"
}

prepare_release() {
  local version=""
  local base_ref="HEAD"
  local branch_name=""
  local create_branch="yes"
  local release_date

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --base)
        [[ $# -ge 2 ]] || die "--base requires a git ref"
        base_ref="$2"
        shift 2
        ;;
      --branch)
        [[ $# -ge 2 ]] || die "--branch requires a branch name"
        branch_name="$2"
        shift 2
        ;;
      --no-branch)
        create_branch="no"
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        [[ -z "${version}" ]] || die "Unexpected argument: $1"
        version="$1"
        shift
        ;;
    esac
  done

  [[ -n "${version}" ]] || die "prepare requires a version"
  version="$(normalize_version "${version}")"
  validate_version "${version}"
  ensure_repo_files
  require_clean_worktree

  branch_name="${branch_name:-release/v${version}}"
  release_date="$(date -u +%F)"

  if [[ "${create_branch}" == "yes" ]]; then
    ensure_branch "${version}" "${base_ref}" "${branch_name}"
  fi

  set_manifest_versions "${version}"
  prepare_changelog "${version}" "${release_date}"
  refresh_lockfiles
  check_release "${version}"

  info "Release ${version} is prepared"
}

notes_release() {
  local version="$1"
  local notes

  version="$(normalize_version "${version}")"
  validate_version "${version}"
  ensure_repo_files

  notes="$(extract_notes "${version}")"
  [[ -n "${notes}" ]] || die "No changelog entry found for ${version}"
  printf '%s\n' "${notes}"
}

main() {
  local command="${1:-}"

  case "${command}" in
    prepare)
      shift
      prepare_release "$@"
      ;;
    check)
      shift
      [[ $# -eq 1 ]] || die "check requires exactly one version"
      check_release "$1"
      ;;
    notes)
      shift
      [[ $# -eq 1 ]] || die "notes requires exactly one version"
      notes_release "$1"
      ;;
    -h|--help|"")
      usage
      ;;
    *)
      die "Unknown command: ${command}"
      ;;
  esac
}

main "$@"
