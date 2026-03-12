# A CLI for Hubuum

This CLI interface against [Hubuum](https://github.com/hubuum/hubuum) is still in pre-release state and under heavy development.

## Release workflow

Add release notes under `## [Unreleased]` in `CHANGELOG.md`, then run `scripts/release.sh prepare <version>` from a clean `main` worktree. The script accepts either `0.0.2` or `v0.0.2`, creates `release/v<version>` by default, updates the package versions and lockfiles, rolls the unreleased changelog notes into a dated release entry, and runs the local release checks before you open the release PR.

After the release branch is reviewed and merged, switch back to `main` and run `scripts/release.sh tag`. GitHub Actions uses the same release helper structure to validate the tag metadata and publish the versioned release notes from `CHANGELOG.md`.

For the full step-by-step release process, see [RELEASE.md](RELEASE.md).
