# Maintain this documentation

The public site is <https://hubuum.github.io/hubuum-cli/>. It shares its theme,
version archive, link checks, and publishing workflows with the other Hubuum
projects through [hubuum/.github](https://github.com/hubuum/.github).
`zensical.toml` owns this project's navigation and source-file imports.

## Build and preview

Use Python 3.11 or newer, Bash, Git, and Docker:

```sh
bash scripts/docs.sh build
bash scripts/docs.sh serve
```

The launcher fetches the exact shared-tooling commit in `.github/docs-tools.env`.
The preview is served on `http://127.0.0.1:8000/`. Rebuild after editing source
files. Generated files and caches stay in `target/`. Imported root documents are
copied only into the build directory; edit their original repository files.

## Release documentation

The site root opens the latest stable release. Release paths are `/vX.Y.Z/`;
`/main/` is explicitly selected development documentation. Every release edition
uses its tagged prose. Publishing a new release or updating development keeps older
editions intact, and a moved release tag is rejected.

To add an older published release, run **Actions → Documentation → Run workflow**
on `main` and enter its stable `vX.Y.Z` tag. Backfilling does not change the latest
release default. Each Hubuum project versions its documentation independently.

PRs run strict builds and retain a downloadable `documentation-site` artifact.
Main/release workflows publish through GitHub Actions Pages. The `gh-pages`
branch retains generated snapshots independently of artifact expiry; never
force-push or delete it. The shared setup script enables Pages once per repo.

## Make changes

Add every `docs/**/*.md` page to navigation exactly once. Use relative Markdown
links within the library; links to repository files are rewritten for the site
and pinned to the selected release. Keep examples and compatibility records
with the project that owns them. Shared branding and ecosystem navigation are
maintained centrally; update the pinned tooling SHA and reusable-workflow SHAs
together to adopt a reviewed shared change.
