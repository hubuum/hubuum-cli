# Server compatibility

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
A CLI release targets the server release declared and reproducibly tested by its
bundled `hubuum_client` version. This is a compatibility target, not a guarantee
that every CLI command is available against other server versions.

## Compatibility matrix

| CLI version | `hubuum_client` | Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.11 (upcoming) | 0.10.1 | 0.0.14 | Release target; backup format 5, queued restores, and restorable follow-up backups |
| 0.0.10 | 0.9.1 | 0.0.9 | Current published target |
| 0.0.9 | 0.9.0 | 0.0.9 | Previous declared target |
| 0.0.8 | 0.8.0 | 0.0.8 | Previous declared target |
| 0.0.5 | 0.7.2 | 0.0.5 | Previous declared target |
| 0.0.4 | 0.7.1 | 0.0.4 | Previous declared target |
| 0.0.3 | 0.6.1 | 0.0.3 | Previous declared target |
| 0.0.2 | 0.5.1 | 0.0.2 | Previous declared target |
| 0.0.1 | 0.4.0 | `main@eed194f2339ce221ef251a14062e2a37850186b1` | Historical pre-release snapshot; no stable server target was declared |

The v0.0.10 target is tested by `hubuum_client` v0.9.1 against the immutable
Hubuum server v0.0.9 image
`ghcr.io/hubuum/hubuum-server@sha256:1f12baf882b6d3df5b4b2dbdf26aad0793274e57f86a2c186b8e1e68632db5db`.
The v0.0.9 target is tested by `hubuum_client` v0.9.0 against the same immutable
Hubuum server v0.0.9 image
`ghcr.io/hubuum/hubuum-server@sha256:1f12baf882b6d3df5b4b2dbdf26aad0793274e57f86a2c186b8e1e68632db5db`.
The v0.0.8 target is tested by `hubuum_client` against the immutable
Hubuum server v0.0.8 image
`ghcr.io/hubuum/hubuum-server@sha256:850bfd95a2802485f93c1700fbff5a33465cbc7855cbc94962982c1074fd96f6`.
The v0.0.5 target is tested by `hubuum_client` against the immutable
Hubuum server v0.0.5 image
`ghcr.io/hubuum/hubuum-server@sha256:6f3e0f0debd418acd5cbc2b1399db9859a85ca1fa397525a5ef0e2f493a77c9b`.
The v0.0.4 target is tested by `hubuum_client` against the immutable Hubuum
server v0.0.4 image
`ghcr.io/hubuum/hubuum-server@sha256:60142d605f423b1dc58d9dfe709164b0d5ec93befd2d702f9bdca7ee0654a583`.
The v0.0.3 target is tested by `hubuum_client` against the immutable
server image
`ghcr.io/hubuum/hubuum-server@sha256:f1f57a991f69005ee81f24e77533e61f75b5586949d98cccf1c40fc4329eb186`.
The v0.0.2 target was tested by `hubuum_client` against the immutable server image
`ghcr.io/hubuum/hubuum-server@sha256:8f543383b422124546c8d337fd557e1b182b1b6c7078d7870d3c5cd4f955ef1f`.
The v0.0.1 row records the reproducible server snapshot inherited from
`hubuum_client` v0.4.0; it predates the first stable CLI/server compatibility target.

Forward-compatibility checks against the server's `main` branch are useful early
warnings, but they do not change a published CLI release's declared target.

## CLI v0.0.11: server v0.0.14 target

The release target pins `hubuum_client` 0.10.1 and the immutable server image
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
