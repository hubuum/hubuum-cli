# Release Guide

This project releases from a release branch and publishes binaries when the merge commit is tagged as `v<version>`.

The release helper layout intentionally mirrors the one used in the main `hubuum` server repo:

- `scripts/release.sh` is the maintainer entrypoint
- `scripts/check-release-readiness.sh` validates release metadata
- `scripts/check-version-bump.sh` gates version bumps on changelog updates
- `scripts/extract-changelog-section.sh` prints the changelog notes used for the GitHub release

## Versioning

- The current released version is `v0.0.1`.
- The script accepts either `0.0.2` or `v0.0.2`.
- Release tags must use the `v` prefix, for example `v0.0.2`.
- `Cargo.toml` and `CHANGELOG.md` must use the same version number without the `v` prefix.

## Prepare a release

1. Start from a clean worktree on the branch you want to release from.
2. Add the upcoming release notes under `## [Unreleased]` in `CHANGELOG.md`.
3. Run:

```bash
./scripts/release.sh prepare 0.0.2
./scripts/release.sh prepare v0.0.2
```

By default this will:

- Create `release/v0.0.2`
- Update the package versions in `Cargo.toml` and `cli_command_derive/Cargo.toml`
- Refresh the tracked `Cargo.lock` files
- Move the `Unreleased` changelog notes into `## [0.0.2] - <today>`
- Validate that the release metadata is consistent

If you want to prepare from a different ref, use:

```bash
./scripts/prepare-release.sh prepare 0.0.2 --base origin/main
```

## Review the release branch

After the script runs, review the generated changes:

```bash
git diff
./scripts/check-release-readiness.sh
./scripts/check-version-bump.sh
cargo check --locked
```

Open a pull request from `release/v0.0.2` and merge it after review.

## Publish the release

After the release branch is merged, tag the merge commit from `main`:

```bash
git switch main
git pull
./scripts/release.sh tag
git push origin main v0.0.2
```

GitHub Actions will then:

- Verify that the tag, `Cargo.toml`, and `CHANGELOG.md` all match
- Build release binaries
- Publish the GitHub release using the matching changelog section as release notes

## Useful commands

Print the release notes that GitHub Actions will publish:

```bash
./scripts/extract-changelog-section.sh 0.0.2
./scripts/extract-changelog-section.sh v0.0.2
```

Validate an already-prepared release without changing files:

```bash
./scripts/check-release-readiness.sh
./scripts/check-release-readiness.sh v0.0.2
```

Low-level branch preparation is still available through `./scripts/prepare-release.sh` if you need custom base refs or branch names.
