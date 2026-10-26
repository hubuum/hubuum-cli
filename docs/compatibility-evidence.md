# Compatibility evidence and history

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
Each CLI release pins a `hubuum_client` version; that client library declares
and reproducibly tests its Hubuum server target. CLI v0.0.22 uses
`hubuum_client` 0.20.0, which targets Hubuum server v0.0.21.
The CLI inherits this compatibility target from its client dependency and runs
its own integration checks against the same pinned server. Command availability
against other server versions is not guaranteed.

## Compatibility matrix

| CLI version | `hubuum_client` dependency | Client's Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.22 | 0.20.0 | 0.0.21 | Published SDK and immutable server verified on 2026-10-26; complete discovery, suggestion, and restore workflows passed |
| 0.0.21 | 0.19.0 | 0.0.20 | Flexible history times, timeline comparisons, and clearer detail rendering |
| 0.0.20 | 0.19.0 | 0.0.20 | Related-object list filters, richer pipelines, structured output, and validated SDK IDs |
| 0.0.19 | 0.17.0 | 0.0.20 | Persistent sessions, historical views, credential activity, hydration, and observation controls |
| 0.0.18 | 0.16.0 | 0.0.20 | Rotating interactive sessions and backup format 10 |
| 0.0.17 | 0.15.1 | 0.0.19 | Previous release; structured JSON command diagnostics |
| 0.0.16 | 0.15.1 | 0.0.19 | Previous CLI release; command tracing and adaptive grouped table headers |
| 0.0.15 | 0.15.0 | 0.0.19 | Previous CLI release; query usage, system notifications, structured streaming, backup output 9 and restore input 6/7/8/9 |
| 0.0.14 | 0.14.1 | 0.0.18 | Previous CLI release; collection-owned integrations, direct sink grants, backup output 8 and restore input 6/7/8 |
| 0.0.13 | 0.13.0 | 0.0.17 | Previous CLI release; chat webhook presets, notification policies, backup output 7 and restore input 6/7 |
| 0.0.12 | 0.12.0 | 0.0.16 | Previous CLI release; structured search, credential approvals, task discovery, and backup format 6 |
| 0.0.11 | 0.10.1 | 0.0.14 | Previous CLI release; backup format 5, queued restores, and restorable follow-up backups |
| 0.0.10 | 0.9.1 | 0.0.9 | Previous CLI release |
| 0.0.9 | 0.9.0 | 0.0.9 | Previous CLI release |
| 0.0.8 | 0.8.0 | 0.0.8 | Previous CLI release |
| 0.0.5 | 0.7.2 | 0.0.5 | Previous CLI release |
| 0.0.4 | 0.7.1 | 0.0.4 | Previous CLI release |
| 0.0.3 | 0.6.1 | 0.0.3 | Previous CLI release |
| 0.0.2 | 0.5.1 | 0.0.2 | Previous CLI release |
| 0.0.1 | 0.4.0 | `main@eed194f2339ce221ef251a14062e2a37850186b1` | Historical pre-release snapshot; no stable server target was declared |

CLI v0.0.10 uses `hubuum_client` v0.9.1, tested against the immutable
Hubuum server v0.0.9 image
`ghcr.io/hubuum/hubuum-server@sha256:1f12baf882b6d3df5b4b2dbdf26aad0793274e57f86a2c186b8e1e68632db5db`.
CLI v0.0.9 uses `hubuum_client` v0.9.0, tested against the same immutable
Hubuum server v0.0.9 image
`ghcr.io/hubuum/hubuum-server@sha256:1f12baf882b6d3df5b4b2dbdf26aad0793274e57f86a2c186b8e1e68632db5db`.
CLI v0.0.8 uses `hubuum_client` v0.8.0, tested against the immutable
Hubuum server v0.0.8 image
`ghcr.io/hubuum/hubuum-server@sha256:850bfd95a2802485f93c1700fbff5a33465cbc7855cbc94962982c1074fd96f6`.
CLI v0.0.5 uses `hubuum_client` v0.7.2, tested against the immutable
Hubuum server v0.0.5 image
`ghcr.io/hubuum/hubuum-server@sha256:6f3e0f0debd418acd5cbc2b1399db9859a85ca1fa397525a5ef0e2f493a77c9b`.
CLI v0.0.4 uses `hubuum_client` v0.7.1, tested against the immutable Hubuum
server v0.0.4 image
`ghcr.io/hubuum/hubuum-server@sha256:60142d605f423b1dc58d9dfe709164b0d5ec93befd2d702f9bdca7ee0654a583`.
CLI v0.0.3 uses `hubuum_client` v0.6.1, tested against the immutable
server image
`ghcr.io/hubuum/hubuum-server@sha256:f1f57a991f69005ee81f24e77533e61f75b5586949d98cccf1c40fc4329eb186`.
CLI v0.0.2 uses `hubuum_client` v0.5.1, tested against the immutable server image
`ghcr.io/hubuum/hubuum-server@sha256:8f543383b422124546c8d337fd557e1b182b1b6c7078d7870d3c5cd4f955ef1f`.
The v0.0.1 row records the reproducible server snapshot inherited from
`hubuum_client` v0.4.0; that client did not declare a stable server target.

Forward-compatibility checks against the server's `main` branch are useful early
warnings, but they do not change the server target of a published client version.

## CLI v0.0.22: discovery and schema suggestions

The release uses published `hubuum_client` 0.20.0 against Server v0.0.21.
Immutable image: `ghcr.io/hubuum/hubuum-server@sha256:a5e1ad7a17281eefb4c1ed4da172a80bf815840f78bde6390d1e42bb2d51e0b6`.
Verified on 2026-10-26 with the published SDK and the immutable server image.
The lockfile records the crates.io checksum, and the built CLI uses registry
source from SDK commit `170f4c9e4d181eea14283267ee9c61ee068328c8`, with no local
path patch. All 964 workspace tests passed; two platform-specific native-keychain
cases remain explicit CI checks. Strict workspace Clippy, formatting, a fresh
executable build, and the 456-dependency audit passed.

The canonical backup/restore driver passed sessions and concurrent processes,
computed-cache behavior, query observations, notifications, history, graph
limits, structured pipelines, credential approvals, field inventory, and all
three suggestion presets through explicit stage/impact/activation. It also
passed schema operations, with- and without-history recovery, and
second-generation restoration after further mutations.

`class fields` and REPL completion use bounded server inventories without fetching
object payloads. Structured paths distinguish literal keys from array traversal.
Suggestion commands cover three presets, task resumption, review, staging,
whole-class impact, activation, and revalidation. Machine-format diagnostics use
stderr so stdout and redirected files stay parseable; debug JSON retains its
embedded diagnostic object. See the migration notes in the release changelog.

The release also includes tunable output themes from PR #141. Offline checks
verified that `theme set --preview` leaves configuration unchanged, saving
persists the selected foreground/background overrides, and JSON remains unstyled
even with `--color always`. Workspace regressions cover value classification,
inherited styles, nested JSON formatting, and neutral continuation indentation.
Existing CLI configuration and theme files require no migration; Rust consumers
of the theme crate must follow the breaking API notes in the changelog.

## CLI v0.0.21: history selectors and comparisons

Verified on 2026-10-24 with published `hubuum_client` 0.19.0 against the same
immutable server v0.0.20 image as CLI v0.0.20:

```text
ghcr.io/hubuum/hubuum-server@sha256:eaf59f8a4b4793030a78bc1637a8e2b9fad2f810b8621ff9a874dca534b9a23a
```

The disposable integration suite now creates a class with three revisions and
an object with 253 revisions. Distinct payloads verify complete history across
the 250-entry timeline and snapshot batch boundaries. Live checks cover:

- Every shortened UTC precision from year through second for `history show`,
  `history class`, and `history object`, including empty windows and totals.
- Exact RFC3339 boundaries and equivalent timezone offsets, plus `now`, signed
  and unsigned seconds, and compound relative-time syntax. Relative reads are
  checked against the returned validity interval and the command's clock bounds.
- Previous-entry and latest-entry comparisons outside a selected time window,
  in both navigation directions and across actual server cursors.
- Complete timeline pagination, explicit creation/deletion summaries, deleted
  snapshots in date windows, and a deleted latest comparison reference.
- Text comparison output and JSON pipelines preserving the timeline envelope,
  pagination metadata, comparison direction, and reference history IDs.

The complete pinned suite also passed its existing session, delegated integration,
related-object filtering, schema, query-usage, backup, restore, and recovery checks.

The client dependency, blocking-only features, server target, OpenAPI surface,
backup output 10 / restore input 6–10, and saved-session formats are unchanged.
No dependency was added or upgraded. The CLI still declares no MSRV;
`hubuum-session` retains Rust 1.89 and the SDK retains Rust 1.88. No migration
is required when upgrading from CLI v0.0.20.

Local verification used Rust 1.98.0 and Python 3.12. The workspace suite passed
922 tests, with two opt-in native-keychain tests reserved for platform CI.
Required and workspace-wide Clippy, rustfmt, tracked Markdown lint, strict
documentation build and rendered-link checks, session MSRV, CI-policy tests,
wrapper tests, and extension-schema validation passed. The current RustSec and
registry audit reported no advisories or yank warnings for 449 dependencies.

<!-- markdownlint-disable-next-line MD033 -->
<span id="unreleased-client-v0190-and-related-object-lists"></span>

## CLI v0.0.20: client v0.19.0 and related object lists

Verified on 2026-10-24 with published crates.io `hubuum_client` 0.19.0, checksum
`1e26a5598f35bd7fc547ba8601cbcb29a8c921af4b8909f031a9995ea3b1a0e3`.
Only the SDK package changed in `Cargo.lock`; no other direct or transitive
package was upgraded. Its blocking-only feature selection and Rust 1.88 MSRV
are unchanged. The CLI still declares no MSRV, and `hubuum-session` retains
Rust 1.89. Server v0.0.20, its OpenAPI routes, backup output 10 / restore input
6–10, and persistent-session formats remain unchanged. No server or cache
migration is needed.

The immutable server image used for the complete live CLI suite was:

```text
ghcr.io/hubuum/hubuum-server@sha256:eaf59f8a4b4793030a78bc1637a8e2b9fad2f810b8621ff9a874dca534b9a23a
```

The SDK's [0.19 migration guide](https://github.com/hubuum/hubuum-client-rust/blob/v0.19.0/docs/migration-0.19.md)
describes its breaking Rust APIs. CLI adapters now construct checked IDs,
retain typed relation exclusions, read computed revisions explicitly, and
avoid default/sentinel IDs in graph filtering and fixtures. **Breaking input
validation:** replace zero or negative resource IDs in scripts and fixtures
with real positive IDs. Invalid IDs in server responses also fail deserialization;
computation revision zero remains valid.

The functional addition from SDK 0.18 is related-object filtering for class
object lists. `object list` now exposes it through named groups, exact names or
IDs, bounded depth, and field predicates. The query workspace crate owns
validation. Typed SDK helpers encode plain-list group selectors and depth;
validated dynamic predicates use the existing query-filter representation.
The SDK computed-object cursor lacks `related()`, so a small adapter supplies
the equivalent parameters. Mock transport tests check both paths, including
all-page fetches and locally sorted computed results. Existing structured
search remains useful for OR and whole-predicate negation.

The pinned live run verified direct versus depth-two traversal, name/ID class
selectors, JSON target filters, predicates matching the same target, distinct
groups matching different targets, self-exclusion, computed output and sorting,
total counts, cursor continuation, all-page output, and semantic pipelines.
It also passed the complete existing delegated integration, session, history,
credential, schema, query-usage, backup, restore, and recovery checks.

Verification used Rust 1.98.0 and Python 3.12:

- `cargo test --locked --workspace`: 897 passed; two opt-in native-keychain tests
  remain ignored because no native credential service was available.
- `cargo clippy --all-targets -- -D warnings` and workspace-wide Clippy passed.
- `cargo fmt --all -- --check` and Markdown lint for all tracked Markdown passed.
- `cargo +1.89.0 check --offline --locked -p hubuum-session` passed.
- `python3.12 scripts/test-backup-restore.py --runtime podman` passed against the
  pinned image above after building with the registry SDK.
- CI-policy tests (18), wrapper tests, and shared documentation checks (22) passed.
- The current RustSec audit and crates.io yank check passed for 449 packages,
  using local temporary Cargo/advisory caches because NFS does not support the
  audit tool's lock files.

Cross-platform release builds and native credential-store checks remain CI
checks; this Linux verification does not replace those platform gates.

## CLI v0.0.19: client v0.17.0

ARM64 musl builds compile vendored C atomic operations inline with
`CFLAGS_aarch64_unknown_linux_musl=-mno-outline-atomics`, avoiding unresolved
GCC outlined-atomic helpers from D-Bus while retaining baseline ARMv8 support.
The pinned Docker build supplies this setting. The required CI gate includes
a native ARM64 static build, linkage inspection, and binary startup check.
Linux keychain test setup installs only required packages noninteractively,
with download retries, network timeouts, and a ten-minute step limit.

<!-- markdownlint-disable-next-line MD033 -->
<span id="unreleased-room-graph-wrapper"></span>

### Room graph verification

The room wrapper resolves the Room and fetches a depth-two graph with
`relation object graph --data`. On 2026-10-23, the live CLI suite verified
the wrapper against the same immutable server v0.0.20 image recorded below.
Fixtures create class and object IDs in opposite orders, connect one Host to
two Jacks, and include an empty Jack and null object data. JSON and text output
preserve placement and facts. Room name, numeric ID, and the optional `B` prefix
each require two CLI invocations; this is not a claim of two HTTP requests or
a measured latency improvement. Graph JSON without `--data` retains its shape.

All 852 workspace tests passed, along with strict Clippy, rustfmt, wrapper tests,
ShellCheck, Markdown lint, extension-schema validation, the strict documentation
build and rendered-link checks, and 18 CI-policy tests. The session crate passed
its Rust 1.89 check, and the dependency audit passed. The SDK pin, server target,
OpenAPI surface, dependencies, feature combinations, and MSRV declarations are
unchanged. Upgrade the wrapper together with a CLI supporting `--data`.

<!-- markdownlint-disable-next-line MD033 -->
<span id="unreleased-client-v0170"></span>

### Session and client verification

CLI v0.0.19 pins published crates.io `hubuum_client` 0.17.0 with checksum
`f61d6e73094a5b698cc2bad0e4c86ef1f024cf945608ae196269b281ea210c7b`.
It replaces the temporary Git dependency described below. The server v0.0.20
target and immutable image remain
`ghcr.io/hubuum/hubuum-server@sha256:eaf59f8a4b4793030a78bc1637a8e2b9fad2f810b8621ff9a874dca534b9a23a`.
Client features remain blocking-only, and backup output remains format 10.
The new commands expose additional existing server v0.0.20 routes; this change
does not require a server upgrade or a cache migration from the preceding
development build.

Persistent stores now live in `crates/hubuum-session`, with explicit identity and
storage options and no application configuration dependency. The crate declares
and passes a Rust 1.89 compile check. It preserves file layouts, native keychain
identifiers, login/refresh locks, permissions, and durable writes. The SDK retains
its Rust 1.88 requirement; the application still declares no MSRV.

On 2026-10-23, all 851 workspace tests passed. Strict workspace/all-target Clippy,
rustfmt, Markdown lint, 18 CI-policy tests, wrapper tests, extension-schema
validation, and the strict documentation build/link check passed. The updated
lockfile passed the RustSec vulnerability audit. Regression coverage includes
direct/deleted-resource history, literal numeric names, complete historical
envelopes through pagination and pipelines, rejected cursor/offset cycles,
directional label reordering, bounded hydration, observation controls, and
logout preserving a newer saved session family. It also covers mixed positional
and named resource/token selectors and suppressed pagination hints for activity
summary pipelines.

The expanded live CLI suite passed against the pinned image above, including
session inspection/logout without fresh login, current/user/service-account
credential activity, shared observation controls, all new historical views,
directional labels, hydration, and all three existing backup/restore cycles.
The deleted-object check fetches a retained snapshot after deletion without
assuming the server emits a separate deletion row. Retained history is not a
complete event ledger.

Both native Linux keychain tests passed in an isolated D-Bus/GNOME Keyring
container: store recreation and concurrent CLI migration/resume/refresh.
The session crate also passed a native musl compile check in Alpine. Native
macOS keychain testing remains configured in CI and was not run on this Linux
workstation. The host lacked a musl C compiler; the Alpine check covered the
extracted crate, while the complete workspace was checked and tested on GNU/Linux.

## Earlier development builds: persistent password sessions

The initial session-persistence fix pinned SDK commit
`ad4bde4fa88a34fc2373ef232bd6cd64d237cea7` from
[SDK PR #124](https://github.com/hubuum/hubuum-client-rust/pull/124).
This temporarily replaced the published 0.16.0 dependency with an immutable Git
source providing the storage interface. It included the SDK main-branch additions
already merged before that fix. The server v0.0.20 target, OpenAPI routes, and
blocking-only feature selection remain unchanged. The SDK keeps Rust 1.88;
the CLI declares no MSRV and uses Rust 1.89's standard filesystem locking API.

The CLI stores rotating sessions in an identity-scoped cache and coordinates
refresh through a stable lock file and atomic durable replacement. Passwords are
not stored. Existing legacy-token migration and explicit PAT/service-token files
remain supported. The SDK validates stored server/client bindings and invalidates
consumed refresh credentials before exchange.

File storage remains the default. Runtime opt-in keychain storage adds
`keyring-core` 1.0.1 and the target-specific Apple Keychain 1.0.2 and D-Bus Secret
Service 1.0.1 stores, using Rust cryptography and vendored libdbus on Linux. The credential pair
is stored directly in the native store, with the same exclusive process lock.
File-to-keychain migration preserves unrelated accounts and stops on storage
failure. Existing Unix cache and lock-file permissions are repaired before use.
No password or plaintext fallback is stored by the keychain backend.
Selecting `none` instead uses the SDK's existing in-memory login API and skips
both persistent stores and legacy-token reuse. It introduces no dependencies or
server API changes. Regressions verify fresh logins, preservation of existing
caches, absence of session files, and explicit token-file precedence.

Verification uses the immutable server image
`ghcr.io/hubuum/hubuum-server@sha256:eaf59f8a4b4793030a78bc1637a8e2b9fad2f810b8621ff9a874dca534b9a23a`.
The live CLI driver covers password login, restart without credentials, and two
concurrent processes refreshing a saved session, alongside the complete existing
integration and restore/recovery suite. The companion SDK's complete pinned
library/consumer suite also verifies client recreation after real access-token
expiry. Workspace regressions cover independent processes, identity isolation,
cache permissions, redaction, malformed caches, and preservation of other accounts.

The keychain follow-up passed 830 workspace tests plus two native Linux tests in
an isolated D-Bus/GNOME Keyring container: credential persistence across recreated
stores, and four CLI processes migrating a file session, resuming without a
password, and coordinating exactly one refresh. The latter verifies the consumed
refresh credential is absent from the native store before exchange and that the
plaintext session entry was removed. macOS native validation is included in CI;
it was not run on this Linux workstation. Strict Clippy, formatting, Markdown,
wrapper and CI-policy checks, dependency audit, and a musl compile check passed.

The memory-only follow-up passed 832 workspace tests, strict Clippy, formatting,
Markdown lint, and CI-policy checks. The full pinned live suite passed again,
including fresh memory-only logins both beside an existing cache and in an empty
data directory; existing credentials remained unchanged and no session files
were created. The preceding keychain commit also passed native macOS and Linux
CI validation.

Concurrent cache-miss login coverage forces two clients to finish their initial
cache checks before either receives a password, for both empty and expired
caches. A stable login lock coordinates the subsequent recheck and login/save;
the regression requires exactly one login and successful requests from both
clients after authentication completes. Separate CLI subprocesses also share
one cold-start login. The pinned live driver covers concurrent empty/expired
cache recovery alongside refresh coordination and memory-only authentication.
All 833 workspace tests passed, as did strict Clippy, formatting, Markdown lint,
18 CI-policy tests, and the complete pinned live suite with three restore cycles.
This changes no dependencies, server API, or MSRV requirements.

## CLI v0.0.18: client v0.16.0

Verified on 2026-10-22 using the published crates.io `hubuum_client` 0.16.0,
with registry checksum
`452c0ac2946af679a220a92eef8c804b3927521428dfb0cb5c1bc08b042312b4`.
The final lockfile uses the registry package, without a local path override.
The SDK release commit is
`74349baca65a957d302ade3319bc4d599ec7369f`.

The complete CLI live suite used the released server v0.0.20 image:
`ghcr.io/hubuum/hubuum-server@sha256:eaf59f8a4b4793030a78bc1637a8e2b9fad2f810b8621ff9a874dca534b9a23a`.
Its source revision is `3bede6e5b815585f3ac5372cc829734b2ae14a68`.
Password login and provider discovery use the new `/api/v1/auth` contract.
Interactive sessions rotate in memory; newly acquired credentials are not
written to the legacy token cache. Explicit PAT files remain supported.

All 819 workspace tests passed, together with strict workspace/all-target
Clippy, rustfmt, CI-policy regressions, extension wrapper tests, extension
schema validation, and Markdown lint. The dependency refresh includes the
published SDK and ICU 2.4; the vulnerability audit passed. `cargo update`
reports no further compatible updates. Upstream exact requirements retain
`psm` 0.1.24 through `pest` and `generic-array` 0.14.7 through `crypto-common`.

The pinned live driver passed collection integrations, delegated permissions,
structured search and streaming, credential approvals, schema evolution,
query usage, and all three format 10 backup/restore cycles. It also verifies
that password login can read administrator configuration without printing the
password or persisting the session, and that second-generation recovery
preserves mutations and prior deletions.

**Breaking upgrade:** upgrade the server API, workers, and restore executor
together during the offline migration, retain a pre-upgrade PostgreSQL snapshot
and binaries, and sign in again. Each new CLI process requires credentials
unless using an explicit PAT file; use the REPL to reuse an interactive session.
Older servers cannot restore format 10 output.

## CLI v0.0.17: client v0.15.1

CLI v0.0.17 keeps the published `hubuum_client` 0.15.1 and server v0.0.19 target.
It adds a version 1 JSON diagnostics response around the original command data.
No dependency, feature, MSRV, server target, or OpenAPI contract changes are
introduced. JSON debug consumers must read results from `.data` and diagnostics
from `.debug`; other output modes keep their existing behavior.

The release checks use the same immutable server image as v0.0.16:
`ghcr.io/hubuum/hubuum-server@sha256:6fbe91693356b8885e6b1115cea4417dbf80747ba4af3eb0a15b855ffff47225`.
Regression coverage includes API failures and HTTP statuses, pipeline and redirect
errors, extension warnings, JSON result shapes, command isolation, and diagnostic
redaction. An authorized read-only Hosts query on 2026-10-19 returned two rows
with three client API calls and three HTTP attempts, all status 200, inside a
single JSON response with empty stderr. All 813 workspace tests passed, together
with Clippy, rustfmt, Markdown lint, wrapper tests, extension schema validation,
and CI-policy tests. The disposable-server integration suite passed against that
pinned image, including all three format 9 restore cycles and the supported
command/streaming surfaces. A fresh audit found no vulnerable dependencies in
the 424-package lockfile.

## CLI v0.0.16: client v0.15.1

CLI v0.0.16 pins the published `hubuum_client` 0.15.1 for its versioned tracing
contract. The server target remains v0.0.19, with immutable image
`ghcr.io/hubuum/hubuum-server@sha256:6fbe91693356b8885e6b1115cea4417dbf80747ba4af3eb0a15b855ffff47225`.
The 240-operation OpenAPI contract and 111 wire-model mappings are unchanged.
The CLI still enables only the client's blocking feature; the client retains
Rust 1.88 as its MSRV, and the CLI declares no MSRV. `tracing` 0.1 is now a direct
CLI dependency, using the version already in its lockfile.

On 2026-10-19, all 808 workspace tests passed using the crates.io package with
its registry checksum locked. Clippy, formatting, tracked Markdown lint, wrapper
tests, extension JSON Schema validation, and CI-policy tests passed. Diagnostics
tests verify real HTTP responses, failure statuses, span ancestry, cancellation
boundaries, command isolation, and clean JSON/redirection. An authorized read-only
live query also confirmed four client calls and four native HTTP attempts.
Verification used Rust 1.98.0. A refreshed advisory audit found no vulnerable
dependencies in the final 424-package lockfile.

The complete disposable-server integration suite passed against the pinned image
with the registry client, including delegated integrations, query usage,
structured streaming, graph limits, credential approvals, schema evolution, and
all three format 9 restore cycles. Recovery retained object and query-usage
revisions, JSON nulls, and earlier deletions; old tokens were invalidated.

Command diagnostics collect the client's version 1 tracing contract beneath
one root per command, with separate API-call and HTTP-attempt counts. Startup
and detached background activity are excluded. Reports use stderr so structured
output and file redirection keep their existing formats.

**Breaking for extension authors:** `debug` and `--debug` are reserved host
options. Rename any conflicting extension option before upgrading; executable
extensions do not receive this host flag.

## CLI v0.0.15: client v0.15.0

CLI v0.0.15 pins the published `hubuum_client` 0.15.0 and the released
server v0.0.19 image
`ghcr.io/hubuum/hubuum-server@sha256:6fbe91693356b8885e6b1115cea4417dbf80747ba4af3eb0a15b855ffff47225`.
The upstream contract grows from 235 to 240 operations and has 111 reconciled
wire-model mappings. Client features remain blocking-only and its MSRV remains
Rust 1.88. The CLI declares no MSRV; local verification used Rust 1.98.0.

On 2026-10-19, all 799 workspace tests, Clippy, formatting, tracked Markdown lint,
wrapper tests, extension JSON Schema validation, CI policy tests, and documentation
build/link checks passed. Cargo audit found no vulnerable dependencies using a
fresh advisory database; its optional registry refresh could not lock the local
NFS cache. The client itself was fetched from crates.io and its checksum is locked.
The unchanged workspace library APIs introduce no semver or feature changes.

`cargo build --locked` followed by `python3.12 scripts/test-backup-restore.py`
passed against the pinned image using isolated Podman storage under `/tmp`.
This verifies the Linux amd64 image from the multi-platform index. The suite
covered collection-owned integrations, shared webhook presets, CLI preview/test,
system subscription CRUD and authentication subject filters, relation-aware audit
including deleted relationships, object/relation reads and graph limits, typed
structured search and incremental JSON/JSONL streams, resource pipelines,
credential approvals, task discovery, and schema evolution. Query-usage CRUD, stale
revision rejection, and hypothetical analysis without declaration mutation passed.
All three format 9 restore cycles completed, including with/without history and a
second generation.
Each cycle recovered a deleted query-usage declaration with its revision and
attribution, invalidated old tokens, and preserved object revisions, timestamps,
and JSON nulls. Follow-up staging and preservation of an earlier deletion passed.
Unit tests retain format 6, 7, and 8 acceptance and reject unsupported versions
before row decoding. See [upgrade and recovery](backup-restore.md).

## CLI v0.0.14: client v0.14.1

CLI v0.0.14 pins the published `hubuum_client` 0.14.1, which targets Hubuum
server v0.0.18. The client reconciles all 235 operations and 338 schemas, with
88 wire-model mappings. CLI integrations use the same immutable released server
image recorded in `Cargo.toml`. The client enables only its `blocking` feature
and retains Rust 1.88 as its MSRV; the CLI declares no MSRV and is verified with
Rust 1.99. The Rust client's public API remains compatible with 0.14.0.

Verification on 2026-10-06 used the published crates.io client and released server
image `ghcr.io/hubuum/hubuum-server@sha256:5b54248f19171200dfa497174d385a48f90666a415cb31732797043d5e182fc4`,
from server commit `35fcf6696d4d564e2d89534db0c5194c14129d9f`. All 743 workspace
tests and the complete disposable-server suite passed, including delegated
collection setup, shared chat presets, credential approvals, schema workflows,
and full restore/recovery with and without history plus a second-generation restore.

Collection managers can create, update, and delete fixed-destination webhooks and
subscribe to their collection's events. These operations require both
`ManageEventSubscription` and `ReadAudit`. Global sinks require direct collection
grants; administrators can grant, list, and revoke them through the CLI. The
required pinned integration run covers the delegated lifecycle, administrator
chat presets, credential approvals, and full backup/restore recovery.

**Breaking (preset arguments):** pass chat-provider destination URLs through
`--destination-url-file PATH`; literal destination URL arguments are no longer
accepted. Store those files with restrictive permissions. See
[collection-owned webhooks](webhooks.md#collection-owned-destinations).

**Breaking (server target and backup output):** upgrade the server before using
collection self-service. Stop every writer and take a PostgreSQL snapshot before
the collection-sink migration, then deploy matching v0.0.18 binaries. Recovery
requires that snapshot and matching v0.0.17 binaries. New backups use format 8;
restore input accepts 6, 7, and 8. Older servers cannot restore format 8. See
[upgrade and recovery instructions](backup-restore.md).

## CLI v0.0.13: client v0.13.0

CLI v0.0.13 pins `hubuum_client` 0.13.0, which targets Hubuum server v0.0.17.
`Cargo.toml` records that client's server version and the immutable multi-platform
image used for CLI integration checks:
`ghcr.io/hubuum/hubuum-server@sha256:cc0518167816bfddb38853b8b7217c4a347511318d51e1abca93ca418f31b302`.
The server tag is `v0.0.17`, source commit
`4a03d56b27f35af62175a80d09d36d0d41c4a663`. The client's reviewed OpenAPI
contract grows from 220 operations and 330 schemas to 227 operations and 336
schemas. The [client release evidence](https://github.com/hubuum/hubuum-client-rust/blob/v0.13.0/COMPATIBILITY.md#v0017-target)
records all 87 reconciled wire-model mappings and its own pinned integration,
feature, Rust 1.88, contract, and semver checks.

The CLI continues to enable only the client's `blocking` feature. The client's
MSRV is still Rust 1.88; no CLI MSRV is declared or implied by that requirement.
This release is verified with Rust 1.99.0. Existing workspace public interfaces
are unchanged; the JSONC parser update explicitly retains the existing syntax.
The new `hubuum-update` crate adds a small typed update interface backed by
`self_update` 1.3 (upstream MSRV 1.88), with GitHub, Rustls, tar/ZIP, and checksum
features. It updates official CLI binaries independently of the server. See
[self-update behavior and supported platforms](../README.md#updating-in-place).

[Chat webhook presets](webhooks.md) generate ordinary webhook configuration
for Slack, Mattermost, and Discord. Sink create/update exposes delivery policies;
subscription filter JSON accepts task kinds. Delivery health retains nullable
collection IDs for system subscriptions, and delivery records retain purpose and
deferral metadata. System-subscription CRUD and notification preview/test have
no dedicated CLI commands; use the server API. These limits mean that the CLI
does not expose every operation in the upstream OpenAPI contract.

**Breaking backup output:** new backups use format 7, which older servers cannot
restore. Staging accepts formats 6 and 7; keep existing format 6 files intact.
Restores reset transient sink scheduling while preserving notification
configuration and terminal delivery history. See [backup migration](backup-restore.md).

**Breaking server upgrade:** stop all API, worker, and restore-executor writers,
take a PostgreSQL snapshot, then apply migrations and start matching v0.0.17
binaries. Binary-only rollback is unsupported. Recovery requires that snapshot
and matching v0.0.16 binaries, losing subsequent writes. Optional Treetop
installations must upgrade to protocol 0.1 and migrate their policy bundles.
See the [server release notes](https://github.com/hubuum/hubuum/releases/tag/v0.0.17).

The reproducible CLI check is
`cargo build --locked && python3 scripts/test-backup-restore.py`.
It passed on 2026-10-05 with Rust 1.99.0 on Linux x86_64 against the pinned
server above and PostgreSQL 18 image
`docker.io/library/postgres:18@sha256:5a5a84b19854a9ffaa54082c166ff4ec27473a361e496e5ea167f298f2da9722`.
The run used isolated Podman storage under `/tmp` and a RAM-backed disposable
database because the host's `/var` filesystem was full. This verifies Linux
amd64 from the multi-platform index, not every architecture.

All three webhook presets were created through the CLI and rendered by the real
server preview endpoint, including `[TEST]` messages, Discord length bounds and
disabled mentions, and pacing updates/clearing. The test does not resolve real
provider secrets or send messages to hosted Slack, Mattermost, or Discord.
Object/relation reads, structured search and JSONL streams, credential approval
rejection and success paths, task discovery, and schema evolution passed.
All three format 7 restore cycles completed with approval, token invalidation,
password reset, and preservation of revisions, timestamps, JSON nulls, and
previous deletions. Follow-up staging after a history-free restore also passed.
Local regression tests cover format 6 and 7 decoding and rejection of unsupported
versions before row decoding.

## CLI v0.0.12: client v0.12.0

CLI v0.0.12 pins `hubuum_client` 0.12.0, which targets Hubuum server v0.0.16.
Its integration checks use the immutable multi-platform server image
`ghcr.io/hubuum/hubuum-server@sha256:37b3299edd845a0c2aa7772d7d68565233ac8c1802bc44be3fb4bbc6dfa8778e`.
The client's pinned OpenAPI contract has 220 operations and 330 schemas, up from
204 operations at CLI v0.0.11. New client features include fresh credential
approvals and typed task discovery. The CLI also exposes structured resource
search through the client's raw request API, with validation owned by the new
`hubuum-search` workspace crate. Client features remain blocking-only; its MSRV
remains 1.88. No CLI MSRV is declared. Local verification uses Rust 1.98.0.

This upgrade includes the schema-command and backup-format changes introduced
by server v0.0.15, plus v0.0.16's credential approval requirement. Follow
[schema evolution](schema-evolution.md),
[backup migration](backup-restore.md), and
[credential approvals](credential-approvals.md). All schema writes move to
`class schema`; the old class create/modify policy flags are removed. Backup
format remains 6 when upgrading from server v0.0.15. Older format 6 artifacts
without task discovery metadata remain accepted.

Drain workers, quiesce credential changes, and run `hubuum-admin --migrate`
before starting upgraded API, administrator, template-worker, and restore-executor
processes together. Server v0.0.16 adds task-discovery and credential-approval
migrations. Keep a verified backup from the previous server version.

The reproducible check is
`cargo build --locked && python3 scripts/test-backup-restore.py`.
Executed successfully on 2026-09-22 with Rust 1.98.0 on Linux x86_64 against the
pinned image above. It verified structured query files and equivalent terminal
predicates, JSONL search events, bearer-only credential rejection, approved local
user creation/password changes, user token creation/renewal, service-account token
creation, credential-import dry runs, and retained backup task discovery. Schema checks covered
incompatible/compatible impact, retained HTML reports, strict activation,
compliance, revalidation, and idempotent cancellation through both routes.
All three format 6 restore cycles passed with fresh approval, invalidated old
tokens, and recovered revision/timestamp/JSON-null state. Follow-up staging and
the second-generation restore preserved earlier deletions. This run verifies
the Linux amd64 image from the pinned multi-platform index.

Mock-transport tests additionally cover bound approval payloads, the original
bearer, server-normalized token expiry, ambiguous-send evidence without replay,
structured cursor preservation and loop rejection, and typed task filters across
pages. Subprocess tests require text and JSONL batches to reach stdout before
the server sends `done`, and verify truncated-stream failure and atomic redirects.
These checks do not imply that every OpenAPI operation has a CLI command or that
other server versions are supported.

The read-path cleanup later included in v0.0.13 was initially checked on 2026-10-03 against the same pinned
server and PostgreSQL images with `scripts/test-backup-restore.py`. It verified
object detail text, full JSON (`--json`), and pipeline projections; object lists;
class details; and object/class relation reads, including related classes in
different collections. The run used isolated Podman storage under `/tmp` and a
RAM-backed disposable database because the host's `/var` filesystem was full.
That earlier check did not change the client dependency, its server target, or API surface.

## CLI v0.0.11: client v0.10.1

CLI v0.0.11 pins `hubuum_client` 0.10.1, which targets Hubuum server v0.0.14.
Its integration checks pin the immutable server image
`ghcr.io/hubuum/hubuum-server@sha256:6c1c8d7316a1f60a02e4505611a44e21030ba678b5b451f5b293a12f2bd87594`
in `Cargo.toml`. The client pins the server's 204-operation OpenAPI contract;
the previous v0.0.9 target had 202 operations. The added structured-search POST
routes have no dedicated CLI commands in this update. Administrative config
output includes the newly modeled storage, database-role, secret-source,
token-hash, tracing, query-budget, and traversal settings. The OpenAPI contract
is unchanged from v0.0.13 apart from the server version. Client 0.10.1 retains
the public APIs and features of 0.10.0.

Backup format 5 excludes password hashes, tokens, and token scopes. Restore
confirmation queues work for the matching `hubuum-admin --restore-executor`.
Upgrade the server, administrator, and template-worker binaries together and run
`hubuum-admin --migrate` before starting the server when upgrading from an older
schema. Server v0.0.14 adds no migration over v0.0.13; update any separately
deployed restore executor to obtain its history-free restore fix. Format 4
backups require a compatible older server. Existing history-free format 5
artifacts can be restored by the fixed executor. See
[migration and recovery](backup-restore.md).

The CLI continues to enable only the client's `blocking` feature. Verification
uses Rust 1.98.0; this update does not introduce a CLI MSRV declaration. The
client's own MSRV remains 1.88, which is not a claim about the complete CLI
dependency graph.

The reproducible CLI integration check is
`cargo build --locked && python3 scripts/test-backup-restore.py`. It provisions
its own pinned PostgreSQL and Hubuum containers, applies migrations, runs the
restore executor, and checks both backup history settings and both restore
confirmation modes. Each cycle verifies receipt-only monitoring, terminal
success, rejection of the old token, administrator password reset, and recovery
of a deleted object with JSON-null data, preserving its revision and timestamps.
It then checks default backup staging immediately after a history-free restore
and performs a full second-generation restore after further updates and a
deletion. CI requires this check before publishing rolling binaries.

Executed on 2026-09-10 with Rust 1.98.0 and Podman 5.8.2 on Linux x86_64 against
the immutable image above. Its source revision is
`0b0aa17f278496a32cc018cfcac56f34a408ccd6` (server v0.0.14). The pin identifies
the multi-platform image index; this live run verifies its Linux amd64 image.
All three restore cycles reached `succeeded`, rejected the old token, and
recovered the object with its original revision, timestamps, and JSON-null data
after password reset. Immediate follow-up staging and the second-generation
restore after updates and a deletion passed. Normal CLI object assignments
preserved sibling fields; excessive path depth and array indices were rejected
without modifying the object.

The server's v0.0.13 history-loss rejection is now a required success case;
the earlier history-free follow-up workaround is no longer needed with the
fixed executor.
