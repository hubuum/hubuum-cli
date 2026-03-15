# Changelog

## [Unreleased]

- Using `show` on an object or class now displays the object's or class's relations. Defaults
  to depth 2 and ignoring self-class relations. This behavior can be configured with the
  `--max-depth` and `--include-self-class` flags.
- Redesigned relationship commands around rooted `relation class` and `relation object`
  workflows that use the newer related-resource endpoints.
- Added class relation traversal support (`list`, `direct`, and `graph`) to match the newer
  object relation interface.
- Switched search and relationship handling to the released `hubuum_client` crate and updated
  the dependency to `0.0.2`.
- Improved relation UX with better nested scope help, depth defaults, object-name completion,
  and resolved relation paths.
- Reduced relation hydration overhead by batching related class-relation lookups instead of
  repeatedly fetching the same relation ids.

## [0.0.1] - 2026-03-12

- Switched the CLI to the published `hubuum_client` crate on crates.io.
- Added GitHub Actions release automation for rolling `main` binaries and tagged `v*` releases.
