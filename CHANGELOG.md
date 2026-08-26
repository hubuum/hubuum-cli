# Changelog

## [Unreleased]

- Graph-producing class, object, and relation commands accept `--limit` to
  override the server's related-resource guardrail. Omitting it preserves the
  server-configured default.

## [0.0.9] - 2026-08-07

- Updated `hubuum_client` to 0.9.0 for Hubuum server v0.0.9. The CLI now
  handles revisioned resources, optional point/list projections, both SQL and
  expanded collection-permission responses, and the revised identity and group
  membership shapes. Token lists accept lifecycle-state filters and token
  output includes lifecycle state and revision. User and service-account tokens
  can be renewed into replacement credentials without mutating the source.
- Computed-field updates and deletes retain their `--revision` safety contract
  by resolving the canonical point resource and sending its strong ETag with
  `If-Match`. Collection ACL grants and token revocations also use conditional
  requests when the server exposes a revisioned resource.
- Portable preference export now applies one bounded RFC 6902 operation to the
  authenticated principal's `hubuum-cli` settings namespace. This atomically
  replaces the CLI snapshot without reading and replacing settings owned by
  other clients. `config remote` displays the stored CLI snapshot and its server
  revision without importing or modifying it.
- Import v2 requests preserve per-item write conditions and computed-field
  inputs. `import submit --collection` also rewrites computed-field class keys
  to the selected existing collection.

## [0.0.8] - 2026-08-05

- Updated `hubuum_client` to 0.8.0 for Hubuum server v0.0.8. Class relation
  creation now accepts forward/reverse template aliases and validated per-side
  object-relation limits; relation output includes both aliases and limits.
  Core graph imports preserve collection, class, object, class-relation, and
  object-relation timestamps and class-relation limits, while export task
  details expose total, query, hydration, and render timings. Typed relation ID
  filters also use the server's canonical query keys.
- Paginated commands now accept `--all` to fetch and buffer every
  remaining cursor page before output pipelines run. Pipelines applied without
  `--all` warn when more server pages are available, and unified streaming
  search rejects the incompatible `--all --stream` combination.
- Added personal command aliases for complete command lines, including pipe
  stages and redirects. Aliases accept an optional one-line description; alias
  lists, root help, and effective-config output use the description instead of
  printing long command pipelines, while `alias show` retains the full command.
  Structured map and list values in text `config show` output are now rendered
  as sorted, indented trees instead of dense JSON strings. Existing
  string-valued aliases remain compatible.
- Added scope-preserving token cloning for users and service accounts, with
  optional expiry overrides and post-create source revocation. Active token IDs
  are available through contextual completion.
- Added configurable grouped, full, or hidden table headers. Automatically
  sized tables now stay within terminal width and wrap aligned cell content,
  including compact dense-table rendering.
- Added `object aggregate`, exposing the server's permission-scoped object
  aggregation with ordered data or computed dimensions, numeric measures,
  pre-aggregation filters, aggregate sorting, cursor pagination, and optional
  total counts. Aggregate dimensions, measures, and data filters complete from
  both class schemas and sampled object fields; inspected fields use the
  configured completion cache lifetime. The existing `G` and `A` pipe stages
  remain local transforms over rows already returned by another command.
- Object-list text and pipeline output now promotes dotted data fields used by
  `--where` into explicit columns by default. Use
  `--include-where-results false` to retain the configured data-column layout.
  REPL completion also resumes normal option suggestions immediately after a
  complete `--where` or `--sort` clause.
- Added tested Bash wrapper examples for Host inventory creation, lookup, and
  placement workflows, plus a loadable personal-alias example for finding
  hosts with outdated kernels.

## [0.0.5] - 2026-07-26

- Updated `hubuum_client` to 0.7.2 for Hubuum server v0.0.5. User and
  service-account token creation now reports the authoritative expiry returned
  by the server, including its materialized default. Administrative
  configuration output also includes the token-retention purge settings added
  by the server.

## [0.0.4] - 2026-07-26

- Updated `hubuum_client` to 0.7.1 for Hubuum server v0.0.4, including sensitive
  header handling that keeps bearer tokens, restore capabilities, and custom
  raw headers out of debug output and HTTP/2 compression tables. Object-data
  patches exceeding the server's 1,000-operation limit are now rejected before
  transport.
- Added `user token show` and `service-account token show` with complete token
  metadata, permission and resource boundaries, and resolved collection, class,
  and object names. Object IDs outside the token's explicitly scoped classes
  are marked `unreachable`. ID resolution follows every server cursor page and
  uses command-local positive and negative caches with bounded per-class object
  lookups.
- Expanded `audit show` with the provenance initiator's principal ID and name
  while preserving the complete provenance object in structured output. Audit
  lists now display the immediate actor kind and complete `user`, `system`, and
  `worker` actor filters.
- Added explicit `group add_service_account` and
  `group remove_service_account` commands for managing service-account group
  membership by name.
- Changed `user set-password` to prompt for the new password by default and
  added `--password-file` for automation, preventing inline passwords from
  being stored in REPL history or trace logs.

## [0.0.3] - 2026-07-23

- Updated `hubuum_client` to 0.6.1 for Hubuum server v0.0.3 and refreshed all
  compatible direct and transitive dependencies. This includes transport
  confinement, redirect prevention, and sensitive diagnostic redaction from
  the client's security-focused 0.6.1 release.
- Added exact-name RFC 6902 object-data patching with optional create-if-missing
  behavior and a bounded retry when concurrent creation returns a conflict.
- Added `--token-file` and `HUBUUM_CLI__SERVER__TOKEN_FILE` authentication for
  non-interactive service-account workflows.
- Added a readable nested `diff` to `audit show` output when both snapshots are
  available. Full `before` and `after` values are available with `--complete`.
  The referenced user and collection names are resolved when still available,
  and the diff is rendered after the event metadata.
- Added `history show` for detailed class or object versions selected by history
  ID or an RFC 3339 as-of timestamp.

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
