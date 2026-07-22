# Changelog

## [Unreleased]

- Updated `hubuum_client` to 0.6.0 for Hubuum server v0.0.3 and refreshed all
  compatible direct and transitive dependencies.
- Added exact-name RFC 6902 object-data patching with optional create-if-missing
  behavior and a bounded retry when concurrent creation returns a conflict.
- Added `--token-file` and `HUBUUM_CLI__SERVER__TOKEN_FILE` authentication for
  non-interactive service-account workflows.
- Added a readable nested `diff` to `audit show` output when both snapshots are
  available. Full `before` and `after` values are available with `--complete`.

## [0.0.2] - 2026-07-18

- Updated `hubuum_client` to 0.5.1 for Hubuum server v0.0.2 and refreshed all
  compatible direct and transitive dependencies.
- Added a compatibility matrix recording the CLI, client-library, and declared
  Hubuum server targets.
- Expanded user list and detail output with proper names, identity scopes,
  provider ownership, management state, and synchronization timestamps.
- Expanded group list and detail output with identity scopes, provider ownership,
  external keys, and synchronization timestamps. Detail labels now expand their
  alignment width when fields exceed the configured minimum padding.
- Added unauthenticated Prometheus metrics retrieval from the default `/metrics`
  route or a runtime-configured path.
- Added shared and personal computed-field list, create, update, delete, preview,
  and rebuild commands, plus computed scopes on object reads.
- Added computed-field JSON Pointer completion from the class schema, falling
  back to observed paths from a cached sample of up to 100 class objects.
- Expanded computed values in object-list text output as compact `S:<key>` and
  `P:<key>` columns instead of a truncated envelope preview.
- Added repeatable, dynamically completed `--computed S:<key>` and
  `--computed P:<key>` selections for object list and show commands, plus
  `--computed all`; computed values remain off by default.
- Added portable per-class computed defaults under
  `output.object_class_computed_fields`, with dynamic config completion and
  explicit `--computed none` overrides.
- Made `S:<key>` and `P:<key>` first-class semantic pipe selectors for object
  list and show output, preserving computed JSON types through pipe stages.
- Added local object-list sorting by `S:<key>` and `P:<key>`, including dynamic
  completion from enabled definitions. Computed sorts run before `--limit` and
  reject server cursors because server v0.0.2 cannot represent that ordering.
- Treats `--limit` as a requested page size while enforcing the Hubuum server
  v0.0.2 maximum of 250. Larger values are truncated with a warning, and generated
  next-page commands use the effective value.
- Renamed class-specific local meta columns to display aliases under
  `output.object_list_class_aliases`; the former config and stored-preference
  name remains readable for compatibility.
- Added administrator backup submission, task inspection, secure download, and
  high-level create commands.
- Added two-step destructive restore staging, status, and confirmation. One-time
  capabilities are kept in owner-only receipt files and confirmation requires
  an explicit `--yes`.
- Extended task-kind filtering and completion with backup tasks. The existing
  administrator configuration dump now includes the server v0.0.2 settings.
- Adapted object JSONPath handling for the refreshed `jsonpath-rust` API.

## [0.0.1] - 2026-07-13

- Added rolling `main` and version-tagged release archives for static musl Linux binaries,
  Apple Silicon macOS, and Windows, with SHA-256 checksums for every artifact.
- Added an offline `version` command for one-shot and REPL use, optional server version
  lookup, and commit-derived SemVer build metadata for rolling `main` binaries.
- Updated all dependencies, including `hubuum_client` 0.4.0, and added authentication
  provider discovery, provider-scoped login, redacted administrative server configuration,
  and opt-in exact totals for supported paginated commands.
- Using `show` on an object or class now displays the object's or class's relations. Defaults
  to depth 2 and ignoring self-class relations. This behavior can be configured with the
  `--max-depth` and `--include-self-class` flags.
- Redesigned relationship commands around rooted `relation class` and `relation object`
  workflows that use the newer related-resource endpoints.
- Added class relation traversal support (`list`, `direct`, and `graph`) to match the newer
  object relation interface.
- Switched search and relationship handling to the released `hubuum_client` crate.
- Improved relation UX with better nested scope help, depth defaults, object-name completion,
  and resolved relation paths.
- Reduced relation hydration overhead by batching related class-relation lookups instead of
  repeatedly fetching the same relation ids.
- Added rendered output redirects with `>` and `>>`, including REPL file path completion and
  support for redirecting piped JSON projections.
- Updated to `hubuum_client` 0.2.0 and made the CLI vocabulary match the current Hubuum API:
  `collection` replaces namespace commands and `export` replaces report commands.
- Added semantic `each:<template>` redirects, aggregate sorting support, themes, and expanded
  pipe DSL help topics.
- Fixed pipeline comparisons being mistaken for redirects, enabled jq-compatible `JQ`
  transforms, included hidden values in broad search, and made direct redirects honor shell
  argument and color-mode behavior.

- Switched the CLI to the published `hubuum_client` crate on crates.io.
- Added GitHub Actions release automation for rolling `main` binaries and tagged `v*` releases.
