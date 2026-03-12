#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/Cargo.toml"

die() {
  echo "$*" >&2
  exit 1
}

manifest_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "${CARGO_TOML}" | head -n 1
}

cd "${REPO_ROOT}"

version="$(manifest_version)"
[[ -n "${version}" ]] || die "Could not determine package version from Cargo.toml"

"${REPO_ROOT}/scripts/prepare-release.sh" check "${version}"

release_ref="${1:-${GITHUB_REF_NAME:-}}"
if [[ -n "${release_ref}" ]]; then
  normalized_ref="${release_ref#v}"
  if [[ "${normalized_ref}" != "${version}" ]]; then
    die "Release ref ${release_ref} does not match Cargo.toml version ${version}"
  fi
fi

echo "Release readiness checks passed for version ${version}"
