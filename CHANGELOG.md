# Changelog

## [Unreleased]

<!-- markdownlint-disable-next-line MD033 -->
<span id="v0-0-20"></span>

## [0.0.20] - 2026-10-24

### Added

- Filter `object list` by related objects with repeatable `--related ALIAS=CLASS`
  or `--related-id ALIAS=ID`, `--related-depth ALIAS=DEPTH`, and
  `--related-where ALIAS.FIELD OP VALUE`. Support up to four groups, depth 1–10,
  ordinary pagination, computed output and sorting, pipelines, and REPL completion.
- Add local typed string predicates `CONTAINS`, `ICONTAINS`, `STARTS_WITH`,
  `ENDS_WITH`, `IEQUALS`, and opt-in `FUZZY`, including negated forms. Existing
  regex filters retain their syntax and meaning.
- Add relative datetime operands `now` and `now-Ns/m/h/d` with one clock sample
  per pipeline application and an injectable library clock.
- Add optional selector diagnostics through `CHECK STRICT` / `CHECK ALLOW` and
  `PipelineSettings`, with available paths and spelling suggestions.
- Add `P ROWS` for projection aligned by original array-member addresses and
  `U selector AS item` for unrolling into a named field without promoting members.
- Add offline `H` / `HELP [verb|topic|CURRENT]` to explain options and the current
  statement without executing its command. The existing `help pipe` topics use
  the same reference; help and diagnostic options support REPL completion.

### Fixed

- Reject related-object JSON paths with empty segments, including doubled dots
  immediately after `data` or `json_data`, instead of silently changing the path.
- Preserve large integers in numeric casts, comparisons, sorting, distinct, and
  representable integer sums. Reuse compiled regexes and cast literals across
  rows and pipeline applications.
- Resolve projection removals against original addresses before compacting arrays,
  and share quote/escape handling across pipeline, predicate, comma-list,
  completion, and redirect scanning.
- Count each input row once per distinct group-key combination, even when an
  array repeats a key. Reject unroll field collisions instead of silently
  overwriting parent values.
- Preserve structured response data through pipelines for version and config
  paths, preference and alias updates, theme previews, issued tokens, task
  output and submission, computed previews and mutations, class/group details,
  and relation graphs. Mutation confirmations expose a semantic message.
- Render legacy JSON responses through the shared pipeline and final renderer.
  Exclude auxiliary text from structured payloads while retaining it in JSON
  debug messages. Theme previews now honor structured output formats.
- Preserve object details and historical context in JSONL, CSV, and TSV views.
  Count empty search results as zero records instead of counting status text.
- Replace obsolete `--limit 0` empty-result examples with a valid unmatched
  name filter; page sizes must be positive.
- Apply pipelines consistently to offline commands, including configuration and
  theme inspection, through the shared catalog command handler.
- Validate task IDs, page sizes, computed-field revisions and patches, export
  scopes, remote-call subjects, and history timestamps before resource lookup.
- Enforce semantic output and extension failure validation during direct JSON
  deserialization as well as through protocol parsing.

### Changed

- Upgrade published `hubuum_client` from 0.17.0 to 0.19.0 and adapt to validated
  resource IDs and computed revisions. **Breaking validation:** zero and negative
  resource IDs now fail locally, including IDs in deserialized server responses.
  Replace invalid or sentinel IDs in scripts and fixtures with real positive IDs.
  Computation revision zero remains valid. The server v0.0.20 target, pinned image,
  OpenAPI routes, blocking-only client features, MSRV declarations, backup formats,
  and saved-session formats are unchanged; no server or cache migration is needed.
- **Breaking filter behavior:** integer-only sums now serialize as integers;
  consumers requiring floating-point values must convert them explicitly.
  Array exclusions in a single `P` stage use original indexes; use separate
  stages for sequential removals. Repeated group keys no longer multiply member
  counts; use `U tags AS tag | G tag | C` to count occurrences. Unroll collisions
  now fail; migrate to `U selector AS item` and select fields under `item`.
- **Breaking DSL/API additions:** `H`, `HELP`, and `CHECK` are reserved stage
  words; search for them with `F H`, `F HELP`, or `F CHECK`. Quote a field named
  `ROWS` after `P` to distinguish it from `P ROWS`. Library callers matching
  `PipeStage`, `PredicateOperator`, or `TypedLiteral` exhaustively must handle
  the added variants. New local text/time predicates are rejected by server
  search with guidance to use a local pipeline.
- Add `strsim` 0.11.1 as a direct `hubuum-filter` dependency for fuzzy matching
  and selector suggestions; this version was already in the workspace lockfile.
  This filtering change does not alter the declared Hubuum server target,
  OpenAPI surface, or MSRV.
- **Breaking pipeline changes:** commands with custom text responses now expose
  their JSON fields to pipelines. Replace filters on rendered labels with field
  selectors such as `VALUE cli_version`, `VALUE token`, or `VALUE message`.
  Composite class/group details, relation graphs, and shared computed lists and
  mutations provide one complete response document; use nested selectors such
  as `class.name`, `group.groupname`, `objects`, `relations`, `definitions`, or `state`.
  Ordinary list-table selectors and unpiped text/JSON response shapes remain
  unchanged, except that `theme preview --output json` now returns the theme
  definition. See [output pipelines](docs/output-pipeline.md#remaining-boundaries).
- **Breaking validation changes:** history `--at` rejects pagination/sort flags;
  named export templates reject ad-hoc relation options; remote calls reject
  selectors unrelated to their subject; related-export specifications reject
  extra colon-separated segments. Remove the inapplicable options from scripts.
  Task IDs and page sizes must be positive, computed updates must include a
  change, and extension responses must satisfy their documented output and
  failure invariants. Valid template exports retain the legacy `--scope` syntax.

## [0.0.19] - 2026-10-23

### Added

- Add `relation object graph --data` to include object data in JSON graph output
  without additional object lookups.
- Inspect or end an existing rotating session with `auth status` and `auth logout`
  without creating a new login. Status omits credentials; logout clears only its
  matching saved session family.
- Add history timelines, exact historical object-name lookup, as-of object
  populations, direct relationship history, and historical shared computed
  values. Preserve retained-history coverage, unavailable endpoint context, and
  pagination metadata in JSON and pipelines.
- Add `me token activity`, `user token activity`, and `service-account token activity`
  with offset pagination and complete issuance, source, retention, and overflow evidence.
- Add `object hydrate` for bounded paths to a target class and
  `admin query-observations show|enable|disable` for the shared runtime override.
- Accept and display directional class-relation labels, preserving their meaning
  when class IDs are reordered. Include session and credential-activity settings
  in `admin config`.
- Disable persistent password-session storage with `--session-storage none` or
  `server.session_storage = "none"`. Rotate credentials only in memory, ignore
  saved sessions and legacy token caches, and leave existing credentials intact.
- Opt into macOS Keychain or Linux Secret Service for rotating login sessions with
  `--session-storage keychain` or `server.session_storage = "keychain"`.
  File storage remains the default. Existing matching sessions move to the
  keychain only after a successful secure write; keychain errors never fall back
  to file credentials. Add the platform-specific keyring credential-store crates.

### Security

- Restore owner-only permissions on existing session caches and lock files before
  use, including caches restored or copied with broader permissions.

### Fixed

- Fix Linux ARM64 static binary linking with native keychain support, keeping
  release archives self-contained.
- Fetch `hubuum-room` placement and object data in one depth-two graph after
  resolving the Room, replacing per-Jack relation and per-Host object lookups.
  **Breaking for wrapper-only upgrades:** upgrade `hubuum-cli` together with the
  wrapper; it now requires `relation object graph --data` support.
- Keep object-relation class labels aligned with their objects when object IDs
  and class IDs have different endpoint orderings.
- Fetch `history show --id` directly instead of scanning a bounded history list.
  Accept numeric class/object selectors so retained history remains accessible
  after deletion. History tables show history IDs, revisions, operations, and
  validity intervals while existing JSON snapshots retain their fields.
- Honor positional object names after history class selectors and positional
  token IDs after credential-owner options. Suppress activity pagination hints
  for summary pipelines while retaining warnings about partial evidence.
- Coordinate concurrent cache-miss logins so processes starting with empty or
  expired caches reuse one session instead of replacing each other's credentials.
  Recheck storage after password entry under a cross-process login lock.
- Restore remembered password logins across CLI restarts and wrapper subprocesses.
  Save rotating sessions in `sessions.json`, isolate server/provider/user identities,
  and coordinate refresh across processes with an exclusive lock and atomic writes.
  Passwords are never stored. Expired, revoked, or uncertain sessions require login.

### Changed

- Upgrade published `hubuum_client` from 0.16.0 to 0.17.0.
  The Hubuum server v0.0.20 target, pinned server image, blocking-only client
  features, and backup format 10 remain unchanged. No cache migration is required.
- Move persistent session storage into the `hubuum-session` workspace crate,
  retaining cache formats, keychain identifiers, and process coordination.
  The crate declares Rust 1.89 for filesystem locking; the CLI still has no
  declared MSRV.

## [0.0.18] - 2026-10-22

### Changed

- **Breaking server target:** pin published Rust SDK 0.16.0 and require Hubuum
  server v0.0.20. Password login and provider discovery use `/api/v1/auth`.
  Upgrade server API, workers, and restore executors together during the offline
  migration, retain a pre-upgrade snapshot, and sign in again.
- **Breaking credential cache behavior:** new interactive sessions stay in
  memory and rotate automatically. Each new CLI process needs credentials;
  use an explicit PAT/service-token file for automation or the REPL for reuse.
  Existing cache entries can still be read and validated; no refresh credential
  is persisted across processes.
- Recognize backup format 10 and restore formats 6–10. Older servers cannot
  restore format 10; use a matching upgraded restore executor.
- Refresh direct/transitive dependencies and GitHub Actions, including jaq-all
  0.4. Adapt the embedded jq runner to its input-filename API.
- Treat SDK session expiry as an authentication failure in interactive recovery.
  Read-only commands can retry once after sign-in; mutations are not replayed.

### Security

- Update `rustls-webpki` to 0.103.16 and `webpki-roots`/`webpki-root-certs` to
  1.0.10 to reject distrusted emSign certificate chains
  ([RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html),
  [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381.html), and
  [RUSTSEC-2026-0383](https://rustsec.org/advisories/RUSTSEC-2026-0383.html)).

### Fixed

- Reuse the class schema fetched by `object list` when choosing automatic data
  columns, avoiding a second class lookup for text and pipeline output.
- Reuse embedded collection metadata for object details and known class metadata
  when listing or deleting class relations. Same-class relations avoid duplicate
  class and object lookups.

## [0.0.17] - 2026-10-19

### Added

- Include versioned, structured command diagnostics in JSON output when `--debug`
  is enabled: elapsed milliseconds, API/HTTP/custom transport counts, and nested
  span timings, outcomes, statuses, routes, retries, and pagination details.
  Direct commands, scripts, the REPL, pipelines, and file redirects share this
  behavior. JSON warnings and errors are included as arrays in the response.

### Changed

- **Breaking for JSON debug consumers:** `--debug --output json` now returns
  `{"data": ..., "debug": ...}` instead of a raw result with a text debug report
  on stderr. Read the original result from `.data` and diagnostics from `.debug`.
  JSON timing ends before final response serialization and writing. JSON output
  without debug and diagnostic reports for other output formats are unchanged.

## [0.0.16] - 2026-10-19

### Added

- Add shared `--debug` diagnostics for command duration and each client-library
  API operation, including its duration and success or failure. Reports use
  command-scoped tracing spans, show execution and rendering time, and collect
  client-library requests, HTTP attempts, retries, pagination, and stream spans.
  HTTP attempts include status codes and are counted separately from client calls.
  Reports go to stderr and work with direct commands, scripts, the REPL, and workflows.

### Changed

- Declare `tracing` 0.1 as a direct dependency for command diagnostics, using the
  version already present in the lockfile. Upgrade the pinned client from 0.15.0
  to 0.15.1 for its versioned diagnostics contract; the declared server target
  remains v0.0.19.
- **Breaking for extension authors:** `debug` is now a reserved host option.
  Rename any extension option named `debug` or using the `--debug` alias before
  upgrading. The flag controls host diagnostics and is not forwarded to
  executable extensions.

### Fixed

- Fit grouped table headers into the space available for each column, keeping
  dotted paths on one line when possible and wrapping between path segments
  when needed instead of always stacking every segment.

## [0.0.15] - 2026-10-19

### Added

- Add administrator `event sink preview` and `test` commands using an audit event
  UUID and subscription ID. Preview renders without delivery; test queues a real
  message and bypasses enabled flags and subscription filters. Test submissions
  are never replayed automatically after ambiguous failures.
- Add `event subscription list|show|create|update|delete --system`, with explicit
  collection/system scope selection, authentication subject filters, and REPL
  completion. System lists return all subscriptions; use local pipelines for
  filtering and sorting.
- Add `audit resource --include-relations` for class and object relationship
  activity, including deleted relations, while retaining entity-only defaults.

- Manage class query-usage declarations with `class query-usage
  list|create|replace|delete|analyze`. Replacement and deletion require an explicit
  reviewed revision. Analysis reports workload evidence, schema compatibility,
  suggestions, native resource ownership and adapter progress without adopting
  suggestions or preparing indexes. See [query usage](docs/query-usage.md).

- Stream structured searches with `search --target ... --stream` or
  `search --query-file ... --stream`. Text results and JSONL event envelopes
  arrive incrementally; JSON, pipelines, and redirects retain complete output.
  Truncated streams and terminal errors fail without replaying partial results.

- Add `self_update.check_on_startup` (default `false`) to optionally announce
  newer stable releases when the interactive shell starts, without installing
  them. Add `self_update.enabled` (default `true`) so managed installations can
  disable all update checks and self-update installation. Both settings support
  TOML configuration, environment variables, and the `config` commands. These
  local settings can be changed without connecting to a Hubuum server.
- Add a positive `--limit` override to class/object show and relation graph
  commands. Omit it to retain the server's configured graph size limit.

- Add the standalone `hubuum-room` wrapper, which lists Room and Jack comments
  and connected Hosts' IP addresses, names, and CPUs in the style of `miami_room`.
  It accepts a Room name or object ID and supports structured output with `--json`.
  Room names may omit an initial `B` when no name or ID matches directly.

### Changed

- Use the client's typed structured-search requests, responses, and SSE decoder.
  Structured resource timestamps now use the client's equivalent UTC `+00:00`
  representation. Terminal predicates, cursor continuation, and local resource
  pipelines remain supported.

- **Breaking (server target):** pin `hubuum_client` 0.15.0 and target the released
  Hubuum server v0.0.19 image. Upgrade matching server processes, replace blank
  principal names before upgrading, and retain a pre-upgrade snapshot and matching
  binaries for rollback. Direct server connections require HTTP/1.1. The client
  retains its blocking feature and Rust 1.88 MSRV; the CLI declares no MSRV.
  The upstream contract grows from 235 to 240 operations.
- **Breaking (backup output):** new server backups use format 9 and preserve
  query-usage declarations. Upgrade the restore server before using new backups.
  Restore staging continues accepting formats 6, 7, and 8 alongside format 9;
  older artifacts need no conversion.

- Shortened installation guidance, pinned the shared Atlas walkthrough, and fixed the extension tutorial’s upgrade step.
- Update the TOML dependency from 1.1.6 to 1.1.8, including fixes for key
  serialization and duplicate-key detection when deserializing tables.
- Refresh Rust dependencies, including JSONPath 1.0.12, self-update 1.3.1,
  Serde JSON 1.0.154, Tokio 1.53.3, SHA-2 0.11.1, Syn 3.0.9, and ZIP 9.0.5
  for updater tests. Refresh pinned GitHub Actions, shared documentation tooling,
  Markdown lint, the Rust 1.99.1 Alpine builder, and the PostgreSQL 18 fixture.
  The client/server upgrade and its migration requirements are described above.

### Fixed

- Fixed explicit `file:///C:/...` value sources on Windows, including paths with
  spaces. File and URL loading remains restricted to options that opt into value sources.

- Check self-update installation directory access before downloading, including
  when the existing executable is writable but its directory is not. Errors give
  the target path and package-manager or administrator guidance. Availability
  checks still work without a writable installation directory. The updater now
  uses the existing `tempfile` dependency at runtime for this access probe.
- Keep `object show --data --path` output as sorted, flattened key/value rows,
  including nested fields under the selected path, instead of embedding JSON
  objects in the text output. Keep distinct values when flattened keys collide
  and preserve selected values with no flattened fields. Explicit `--json`
  output remains available.
- Add `help jsonpath` with syntax, shell quoting, and examples for nested fields
  and arrays. Object command help links to this topic; invalid paths now include
  examples and the help reference alongside the parser error.
- Correct missing JSONPath roots when adding `$` or `$.` produces a valid path,
  with a warning showing the original and corrected expression.
- Honor `object show --output json` consistently with `--json`.

## [0.0.14] - 2026-10-06

### Added

- Manage collection-owned webhooks with `event sink list|show|create|update|delete
  --collection NAME`, including chat presets with `--destination-url-file PATH`.
  Subscription sink lookup and completion use collection-scoped discovery.
- Administrator commands `event sink grant`, `revoke`, and `collections` manage
  direct collection grants for shared sinks through the Rust client.

### Fixed

- Collection destination filters and sorts match the server's searchable metadata.
- **Breaking (preset arguments):** chat presets read credential-bearing URLs with `--destination-url-file PATH`;
  literal destination URLs are no longer accepted as preset arguments. Tokenizer
  trace logs record token counts rather than argument contents.

### Changed

- Refresh compatible Rust dependencies, including Hyper 1.12, h2 0.4.20,
  Jiff 0.2.38, and Zeroize 1.9.1.
- **Breaking (backup output):** Hubuum v0.0.18 writes format 8; restore accepts
  formats 6, 7, and 8. Older servers cannot restore format 8. Keep older artifacts
  for recovery and upgrade all server processes before taking new backups.
- **Breaking (server target):** target Hubuum v0.0.18 through the exact published
  `hubuum_client` 0.14.1 dependency. Stop all writers and take a PostgreSQL
  snapshot before the collection-sink migration. Start matching v0.0.18 binaries;
  rollback requires that snapshot and v0.0.17 binaries. See
  [upgrade instructions](docs/backup-restore.md).

- Collection self-service requires the server `v0.0.18`, and both `ManageEventSubscription` and `ReadAudit`. Existing
  administrator subscription workflows retain v0.0.17 support; only a missing
  scoped discovery route permits the legacy lookup. Permission denials never
  trigger that fallback. The CLI now depends on Rust client `0.14.1`; publish that
  client before publishing this CLI update.

## [0.0.13] - 2026-10-05

| CLI | `hubuum_client` dependency | Client's Hubuum server target | Backup formats |
| --- | --- | --- | --- |
| 0.0.13 | 0.13.0 | 0.0.17 | Writes 7; reads 6 and 7 |
| 0.0.12 | 0.12.0 | 0.0.16 | 6 |

### Changed

- Document compatibility through the pinned client library, generic webhook
  setup, and equivalent full commands for the Slack, Mattermost, and Discord
  presets. CLI help includes the generic webhook setup path.
- Pin `hubuum_client` 0.13.0, which targets Hubuum server v0.0.17, and use its
  immutable server image for CLI integration checks. The upstream contract grows
  from 220 operations and 330 schemas to 227 operations and 336 schemas. Client features remain
  blocking-only and its MSRV remains Rust 1.88; no CLI MSRV is declared.
  Verification uses Rust 1.99. See [pinned integration evidence](COMPATIBILITY.md).
- Refresh direct and transitive Rust dependencies, including Reedline 0.52.1,
  JSONC parser 0.34.0, Tokio 1.53.2, and TLS dependencies. Extension manifests
  retain the same JSONC syntax. Update the pinned Rust toolchain action, shared
  documentation workflow revision, Rust 1.99 builder, and PostgreSQL 18 fixture.
  All other Action pins were checked against their current upstream releases.
- **Breaking (backup output):** server v0.0.17 creates format 7 backups, which
  older servers cannot restore. CLI restore staging now accepts both 6 and 7;
  existing format 6 backups need no conversion. Keep older backups for recovery
  on older servers. Restore resets transient sink scheduling while retaining
  notification configuration and terminal delivery history.
- **Breaking (server upgrade):** stop all API, worker, and restore-executor
  writers and take a PostgreSQL snapshot before applying the notification
  migration. Deploy matching v0.0.17 binaries together. Binary-only rollback
  is unsupported; restore the snapshot with matching v0.0.16 binaries to recover,
  losing later writes. Optional Treetop backends require protocol 0.1 and migrated
  policy bundles. See [upgrade instructions](docs/backup-restore.md).
- **Breaking (delivery health output):** system subscriptions have nullable
  collection IDs. Structured consumers must accept `collection_id: null`.

- Reduce HTTP requests for `object show` by reusing the root object returned in
  the relation graph and resolving collection names together. Object lists,
  class details, search, and relation output also reuse available class and collection
  metadata. Text, JSON, pipeline output, and computed fields retain their content.

- Documentation uses the shared warm Hubuum theme from the ecosystem site's
  unversioned stylesheet, including retained release editions. Future styling
  updates no longer require changes or rebuilds in this repository.

### Added

- In-place binary updates with `self-update` and a metadata-only `--check`,
  available in the CLI and REPL without Hubuum login. The new `hubuum-update`
  workspace crate uses `self_update` 1.3 with Rustls, tar/ZIP extraction, and
  mandatory published SHA-256 verification. Updates use the exact platform
  archive from a strictly newer stable release; restart the CLI afterward.
  See [installation and platform details](README.md#updating-in-place).
- Chat webhook setup with `event sink create --target slack|mattermost|discord`
  and `--url-secret-ref`: normal webhook configuration with provider templates,
  acknowledgements, retry/cooldown rules, and one-second pacing. REPL completion,
  command help, and a [setup guide](docs/webhooks.md) cover secret storage and
  Discord's required `wait=true` URL option. Discord messages disable mentions.
- `--delivery-policy` JSON on sink creation and updates; `{}` clears configured
  pacing. Subscription `--filter` accepts `task_kinds` through the updated client.
  System-subscription CRUD and notification preview/test remain server API
  operations without dedicated CLI commands.

- A walkthrough using the shared Atlas example inventory, with classes, objects,
  relations, and permissions linked to the server-owned import and backup.

- A searchable, versioned documentation site with shared Hubuum navigation,
  automatic GitHub Pages publishing, the latest release as its default,
  immutable release snapshots, and an explicit development edition.

### Fixed

- Documentation tables keep long environment-variable names readable, use wider
  reference layouts, and scroll within the table on small screens. Shared style
  fixes also reach retained release documentation.

## [0.0.12] - 2026-09-23

| CLI | `hubuum_client` | Hubuum server target | Backup format |
| --- | --- | --- | --- |
| 0.0.12 | 0.12.0 | 0.0.16 | 6 |
| 0.0.11 | 0.10.1 | 0.0.14 | 5 |

- Preserve local filters and projections when continuing structured search with
  `next`. Reject excessive boolean expressions regardless of keyword casing
  before recursive parsing, preventing stack overflows from long lowercase
  or mixed-case negation chains.
- Display group names in structured tabular search output, preserve per-kind
  pagination cursors in text search streams, and restrict task-filter completion
  to the supported `equals` operator.
- Release CLI v0.0.12 with `hubuum_client` 0.12.0 and the immutable Hubuum server
  v0.0.16 target (220 OpenAPI operations, 330 schemas). Client features remain
  blocking-only and its MSRV stays at Rust 1.88; no CLI MSRV is declared.
  See [pinned integration evidence](COMPATIBILITY.md).
- Add structured resource search with validated `--query-file` requests or
  `--target`, `--class`, and a quoted `--where` predicate. The terminal DSL supports
  boolean groups, typed comparisons, regex, membership, and null tests, with REPL
  Tab completion for fields, class data paths, operators, and boolean continuations.
  Add sorting, cursor pagination, exact totals, all-page collection, and semantic
  resource rows for pipelines. See [search](docs/search.md).
- Stream plain-text search batches and JSONL events to the terminal as they arrive.
  JSON arrays, pipelines, and redirects remain buffered. **Breaking (stream output):**
  `search --stream --output jsonl` now emits one event envelope per line; update
  consumers to read `event` and `data`. Truncated streams and server error events
  now exit unsuccessfully instead of appearing complete.
- **Breaking (credential operations):** server v0.0.16 requires fresh approval for
  local user creation, password changes, token creation/renewal/cloning, credential
  imports (including dry runs), and restore confirmation. Use an unscoped human
  bearer and enter that human's current password when prompted, or pass the new
  global `--approval-password-file FILE` before the command for automation.
  Stored login passwords are not reused. Service-account tokens cannot approve
  these operations. Approval failures and ambiguous mutation responses are not
  replayed; inspect the reported ID with `auth approval show ID` before retrying.
  See [credential migration](docs/credential-approvals.md).
- **Breaking (import payloads):** submission preserves the full supported graph, including credential and
  integration entries, instead of dropping extended fields. Review existing import
  files before resubmission: previously ignored entries now take effect.
- Add typed `task list --where FIELD equals VALUE` discovery filters and
  comma-separated kind/status sets. `task show` exposes retained targets, options,
  output availability, remote side effects, and schema/rebuild details when present.
  See [task discovery](docs/tasks.md).

- Refresh the pinned PostgreSQL 18 integration fixture image.

- Refresh locked Rust dependencies, including Clap 4.6.7 and Quinn 0.11.12,
  and update Docker build workflow actions and the pinned Rust builder to 1.98.1.

- Resolve collection names in `class show` when the server returns only a collection
  ID. Related object and class queries now accept class and collection names through
  `--where class equals Hosts` and `--where collection equals Inventory`.

- Include server v0.0.15 schema evolution and task cancellation in the v0.0.16
  upgrade. Update locked Rustls to 0.23.45. Administrative output includes schema
  budgets, backup row limits, and task execution timeouts.
- **Breaking (schema commands):** remove `--schema`/`-s` and `--validate`/`-v`
  from `class create` and `class modify`. Use `class schema stage`, `impact`,
  and `activate` for all schema policy changes, including initial setup.
  Activation requires an explicit expected active revision and defaults to
  rejecting incompatible objects. Add revision inspection/abandonment,
  compliance pages, revalidation, work diagnostics, and retained HTML reports.
  See [schema migration examples](docs/schema-evolution.md).
- Add a schema-scope help walkthrough and allow `class schema stage --validate`
  to reuse the active schema. Preview validation changes with impact analysis
  before explicit activation; `--schema null` still explicitly removes a schema.
  `stage --from-revision` copies a previous policy into a new proposal for impact
  analysis and activation, with an optional validation override.
- Schema commands now show concise text summaries by default, including impact
  readiness, counts, and bounded failure groups. Aligned labels follow the
  configured output padding; results, impact, and failure details are grouped and
  indented, with zero-only secondary counters omitted. Full schemas and diagnostics
  remain available through `--output json`, structured formats, and pipelines.
- Complete recent schema task IDs with status/summary labels for `work`, reports,
  cancellation, and activation. Suggestions use a bounded task-list request,
  without downloading diagnostics; task cancellation also gains ID completion.
- Add `task cancel` with validated reasons and optional expected status, plus
  class-scoped `class schema cancel`. Cancellation can remain pending while
  executors finish cleanup; inspect `task show` for cancellation metadata,
  deadlines, unattempted items, terminal reasons, and remote side effects.
  Task filtering/completion includes `schema_validation`.
- **Breaking (server and backup compatibility):** backups now require format 6.
  Restore format 5 artifacts with the matching older server, migrate, then
  create new format 6 backups; no conversion is available. Drain old workers,
  run migrations in a quiet window, and upgrade server, administrator, template
  worker, and restore executor together. Revalidate existing enforced classes,
  add external `CancelTask` permissions, and restart string-sorted pagination.

- **Breaking (example wrapper verbose output):** `hubuum-host --verbose` now
  delegates to `object show --data` with two levels of relations and text output.
  Data keys omit the `data.` prefix and values align to the longest key. Verbose
  output uses the CLI's standard metadata and general relation tree. The ID field
  is omitted, and relations include classes beyond Jacks and Rooms.
  Scripts parsing the previous verbose text must migrate to `hubuum-host --json`;
  JSON output retains its Host and placement structure and takes precedence over
  `--verbose`.

## [0.0.11] - 2026-09-10

- Updated `hubuum_client` to 0.10.1, targeting Hubuum server v0.0.14 and its
  pinned 204-operation OpenAPI contract. The client remains configured with
  its blocking feature. Administrative configuration output includes the new
  storage, database-role, secret-source, token-hash, tracing, query-budget, and
  traversal settings. New structured-search POST routes have no dedicated CLI
  commands in this update.
- **Breaking (backup compatibility):** restores now require backup format 5.
  Restore format 4 artifacts using a compatible older server, then upgrade and
  create fresh backups. Editing a backup's version does not convert it. Format 5
  excludes password hashes, bearer tokens, and token scopes; after restoring,
  reset a local administrator password and issue fresh tokens.
- **Breaking (restore completion):** server v0.0.14 confirmation queues a restore.
  A successful `restore confirm --yes` invocation reports acceptance, not
  completion. Scripts requiring completion must add `--wait` or run the new
  `restore wait --receipt <file>` command. Failed, expired, and timed-out waits
  exit unsuccessfully; a timeout does not cancel the restore. Deploy matching
  server, administrator, and template-worker binaries, run
  `hubuum-admin --migrate`, and start `hubuum-admin --restore-executor` before
  confirming restores. See [backup and restore](docs/backup-restore.md).
- Fixed restore status to work without login after token invalidation, added
  receipt-only completion polling, and preserved server creation timestamps
  when staging backups. Receipt IDs, capabilities, checksums, and status response
  identity are validated; capabilities remain absent from displayed records and
  semantic pipelines.
- Backup and receipt writes now replace files atomically, prepare the destination
  before submitting remote work, preserve the last file on unsuccessful writes,
  and keep a recoverable temporary file if final installation fails. Unix files
  remain owner-only.
- **Breaking (file destinations):** symbolic links and other non-regular backup
  and receipt destinations are rejected even with `--force`; use the regular
  destination path instead.
- **Breaking (pipeline input):** backup-save summaries and restore records now
  supply structured fields to semantic pipelines. Update pipelines that filtered
  formatted lines or flattened key/value rows to select record fields, such as
  `P backup.source_version` or `P status`.
- Added positive `server.max_response_body_bytes` configuration for backups
  larger than the default 16 MiB response limit.
- Adopted server v0.0.14's fix for history-free restores: later default backups
  remain restorable while live revisions and timestamps are preserved. The
  pinned regression check now requires successful follow-up staging and a full
  second-generation restore after further updates and deletions. Existing
  history-free format 5 artifacts can be restored with the matching fixed
  executor. This server release adds no migration over v0.0.13; upgrading alone
  does not repair an already inconsistent database. See the recovery guide.
- **Breaking (object assignments):** upgraded `jqesque` to 0.1. Object
  modification assignments now reject paths deeper than 128 components and
  array indices above 1,000,000. Reduce path depth or array indices in affected
  `object modify --data` commands. Ordinary assignments retain their behavior.
  The updated dependency also resolves the `jsonptr` version conflict, removing
  the need to manually preserve a shared lockfile pin with `json-patch`.
- Refreshed all compatible locked dependencies, including `bitflags` 2.13.2 and
  `jsonpath-rust` 1.0.11, upgraded `dirs` to 7, and refreshed release-action pins.
  CI now checks workspace formatting, tests and Clippy, plus pinned live
  backup/restore recovery. Dependabot tracks
  Cargo and Docker dependencies as well as actions. Verification uses Rust
  1.98.0; no CLI MSRV declaration is added.

## [0.0.10] - 2026-08-30

- Added canonical `class fields --name <class>` field discovery, with
  `object fields --class <class>` retained as a deprecated compatibility alias
  that prints an exact replacement command. Existing invocations continue to
  work; users should migrate to `class fields --name <class>`. Reusable command
  deprecation metadata now annotates help and rewrites renamed options in
  warnings. The inventory includes enabled shared and personal computed
  selectors alongside sampled `data.*` paths, identifies each field's source,
  and summarizes computed values observed in the same object sample. This is a
  breaking output-shape change: the inventory gains a `Source` column and
  computed-selector rows. Structured consumers that assumed an exact column set
  or data-only rows must accept the new column and rows.
- Added extension packs under `extension <pack>`, with dependency-free portable
  JSONC/JQ workflow packs as the preferred kind and a versioned argv/JSON
  protocol for explicitly executable packs. Packs have deterministic system and
  user discovery, catalog help and static completion, semantic output pipelines,
  diagnostics, and local user-package lifecycle commands. The Host, Jack, and
  Room placement example is one portable workflow pack; the Host shell wrappers
  remain a tested executable pack pilot and retain their standalone entry
  points.
  Workflow commands use strict manifest schemas and a catalog contract with
  stable input IDs, portable types and cardinality, explicit effects, early
  compatibility checks, and preserved step-output metadata. Manifest commands,
  options, workflows, and inputs use nested named objects. Workflow steps are
  tagged objects in ordered arrays, keeping IDs, bindings, conditions, and
  dependency references together.
- Expanded the native semantic pipeline with typed boolean `F WHERE` and
  `reject WHERE` predicates, stable multi-key sorting with strict casts and
  explicit null placement, projection aliases and deep exclusions, stable
  `D`/`distinct`, global aggregation, selector counts, and ordered `first` and
  `last` aggregates. Pipeline stages now operate on canonical semantic JSON
  before final rendering, validate their selectors, output names, and shape
  transitions, and apply post-group filters to visible group summaries without
  mutating member rows.
  These are breaking validation changes: malformed selectors, duplicate or
  colliding output names, unsupported stage-shape transitions, and invalid
  values sorted with `AS ip` now fail instead of being tolerated or coerced.
  Users must correct selector syntax, give projected/grouped/aggregate outputs
  unique names, reorder incompatible stages, and filter invalid addresses or
  select a non-IP cast. Existing legacy filter, projection, one-key sort, and
  grouped aggregate forms remain available.
- Interactive REPL sessions now recover from expired or revoked credentials by
  re-reading token files or logging in again. Read-only commands retry once
  after successful authentication; potentially mutating commands are not
  replayed and identify the failed HTTP method and redacted path for review.
  One-shot commands and scripts remain non-interactive.
- Server startup now checks the unauthenticated health endpoint before asking
  for an interactive password. Unconfigured ports are tried in order on 443 and
  8080, while an explicitly configured port remains authoritative.
- Fixed `object show` relation expansion to exclude same-class roots in the
  server request when the default same-class filter is active, preventing
  high-fanout neighbors from hitting the graph-size guard before local
  filtering. Completion now also offers `--class` before the class-dependent
  `--name` option.
- Updated `hubuum_client` to 0.9.1 while retaining the declared Hubuum server
  v0.0.9 target and its pinned immutable integration image. The client update
  adds strict JSON-query-path validation, atomic export downloads, expanded
  OpenAPI model reconciliation, and refreshed dependencies. The complete CLI
  dependency and release-action set was also refreshed, including the `h2`
  update that addresses RUSTSEC-2026-0258.

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
