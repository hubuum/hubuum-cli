# Changelog

## [Unreleased]

- Added a dedicated `RELEASE.md` guide for preparing, validating, and publishing releases.
- Updated the release helper script to accept both `0.0.2` and `v0.0.2` style inputs.
- Synchronized the CLI release helpers with the server repo by adding `release.sh`, release-readiness checks, changelog extraction, and version-bump hygiene checks.

## [0.0.1] - 2026-03-12

- Switched the CLI to the published `hubuum_client` crate on crates.io.
- Added GitHub Actions release automation for rolling `main` binaries and tagged `v*` releases.
