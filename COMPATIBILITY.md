# Client and server compatibility

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
Each CLI release pins a `hubuum_client` version; that client library declares
and reproducibly tests its Hubuum server target. CLI v0.0.14 uses
`hubuum_client` 0.14.1, which targets Hubuum server v0.0.18.
The CLI inherits this compatibility target from its client dependency and runs
its own integration checks against the same pinned server. Command availability
against other server versions is not guaranteed.

## Compatibility matrix

| CLI version | `hubuum_client` dependency | Client's Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.14 | 0.14.1 | 0.0.18 | Current target; collection-owned integrations, direct sink grants, backup output 8 and restore input 6/7/8 |
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
[collection-owned webhooks](docs/webhooks.md#collection-owned-destinations).

**Breaking (server target and backup output):** upgrade the server before using
collection self-service. Stop every writer and take a PostgreSQL snapshot before
the collection-sink migration, then deploy matching v0.0.18 binaries. Recovery
requires that snapshot and matching v0.0.17 binaries. New backups use format 8;
restore input accepts 6, 7, and 8. Older servers cannot restore format 8. See
[upgrade and recovery instructions](docs/backup-restore.md).

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
[self-update behavior and supported platforms](README.md#updating-in-place).

[Chat webhook presets](docs/webhooks.md) generate ordinary webhook configuration
for Slack, Mattermost, and Discord. Sink create/update exposes delivery policies;
subscription filter JSON accepts task kinds. Delivery health retains nullable
collection IDs for system subscriptions, and delivery records retain purpose and
deferral metadata. System-subscription CRUD and notification preview/test have
no dedicated CLI commands; use the server API. These limits mean that the CLI
does not expose every operation in the upstream OpenAPI contract.

**Breaking backup output:** new backups use format 7, which older servers cannot
restore. Staging accepts formats 6 and 7; keep existing format 6 files intact.
Restores reset transient sink scheduling while preserving notification
configuration and terminal delivery history. See [backup migration](docs/backup-restore.md).

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
[schema evolution](docs/schema-evolution.md),
[backup migration](docs/backup-restore.md), and
[credential approvals](docs/credential-approvals.md). All schema writes move to
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
[migration and recovery](docs/backup-restore.md).

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
