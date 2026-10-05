# Backup and restore

CLI v0.0.13 uses `hubuum_client` 0.13.0, which targets Hubuum server v0.0.17.
Backups and restore staging/confirmation require administrator access.

## Prepare the server

**Breaking server upgrade requirement:** upgrading from 0.0.16 requires a
maintenance window. Stop all writers, including API, worker, and restore-executor
processes, then take a PostgreSQL snapshot. Apply the webhook-notification
migration with `hubuum-admin --migrate` before starting matching 0.0.17 binaries.
Binary-only rollback is unsupported: recovery requires the snapshot and matching
0.0.16 binaries, losing writes made after the snapshot. Optional Treetop
installations must also upgrade to protocol 0.1 and compatible policy bundles.
See the [server release notes](https://github.com/hubuum/hubuum/releases/tag/v0.0.17).

When upgrading from before 0.0.15, existing enforced objects start pending;
request schema revalidation. Review
[schema evolution](schema-evolution.md) for policy and authorization migration.

**Breaking backup output change:** server v0.0.17 creates format 7 backups,
including notification configuration and terminal delivery history. Older servers
cannot restore format 7. CLI v0.0.13 and server 0.0.17 still accept format 6 with
legacy notification defaults; keep existing format 6 artifacts intact. Restores
reset transient sink scheduling. Restore format 5 artifacts with the matching
older server, then migrate and take a new backup. Changing `backup_version`
does not convert it. Keep a verified backup from the previous server version.

## Create a backup

```sh
hubuum-cli backup create --file hubuum-backup.json
hubuum-cli backup create --file state-only.json --include-history false
```

Creation waits up to 300 seconds by default. Set `--timeout` and `--poll-interval`
when needed. For task-based automation, retain the ID returned by submission:

```sh
hubuum-cli backup submit --idempotency-key nightly-2026-09-09
hubuum-cli backup show 123
hubuum-cli backup download 123 --file hubuum-backup.json
```

Formats 6 and 7 exclude password hashes, bearer tokens, and token scopes. The manifest
lists excluded data. Privileged integration configuration remains sensitive;
protect the backup accordingly. Saved JSON retains the server's creation instant,
including its UTC offset and fractional seconds, so it can be staged again.

The default HTTP response limit is 16 MiB. To download larger backups, configure
a positive byte count, for example 256 MiB:

```sh
hubuum-cli config set --key server.max_response_body_bytes --value 268435456
```

The same setting is available in TOML or through
`HUBUUM_CLI__SERVER__MAX_RESPONSE_BODY_BYTES`. Restart an existing REPL after
changing it, so its client uses the new limit. Backups are decoded in memory;
allow memory for the parsed document as well as its serialized representation.
The server's restore upload-size limit also applies when staging.

## Stage and confirm

```sh
hubuum-cli restore stage --file hubuum-backup.json --receipt restore-receipt.json
hubuum-cli restore status --receipt restore-receipt.json
hubuum-cli restore confirm --receipt restore-receipt.json --yes --wait
```

Staging validates the artifact without replacing data. Its receipt contains the
restore ID, staged SHA-256, and one-time capability. Keep the receipt private and
retain it until recovery finishes. Use the same configured server for every step.
The CLI never displays the capability in normal output or semantic pipelines.

Confirmation with `--yes` destructively replaces all Hubuum data. The server first
returns `confirmed`, meaning the restore is queued. `--wait` polls until
`succeeded`, `failed`, or `expired`; only `succeeded` exits successfully.
Without `--wait`, confirmation returns immediately after acceptance.

Server 0.0.17 retains the requirement for fresh approval from an unscoped human user before
confirmation. `--yes` still confirms destructive intent, but does not replace
password approval. Interactive sessions prompt for the acting human's current
password. In scripts, place `--approval-password-file FILE` before `restore`
and protect the file with owner-only permissions. After an ambiguous response,
inspect the receipt and approval evidence before attempting confirmation again.
See [credential approvals](credential-approvals.md).

## Resume monitoring and recover access

```sh
hubuum-cli restore wait --receipt restore-receipt.json --timeout 600
hubuum-cli restore status --receipt restore-receipt.json --output json
```

Both commands work without logging in and send only the receipt capability to
the status endpoint. They continue to work after the restore invalidates existing
bearer tokens. `status` displays the current state; `wait` exits unsuccessfully
on failure, expiry, or timeout. Polling defaults to one second and rejects zero
intervals. `--timeout 0` checks the current status once. The polling timeout is
checked between requests; an in-flight HTTP request can add to elapsed time.

A timeout or interrupted CLI does not cancel the server restore. Reuse the same
receipt to resume monitoring. If the state remains `confirmed`, check the
matching restore executor and its logs. Inspect the returned `error` field for a
failed restore.

After `succeeded`, run the following on the server against the restored database:

```sh
hubuum-admin --reset-password admin
```

Substitute a restored local administrator's name when needed. Log in with the
reset password and issue fresh tokens, including replacements for service
accounts and any CLI `--token-file`. Old passwords and tokens are absent from
format 6 and 7 backups. Recheck integration configuration before resuming automation.

## File handling

Backup and receipt files use owner-only permissions (`0600`) on Unix. Existing
files require `--force`. Directories, symbolic links, and devices are rejected;
provide a regular file path. Writes use a temporary file in the destination
directory followed by atomic installation. Failed remote operations leave an
existing destination intact. If installation fails after the content is written,
the error gives the private temporary file path for recovery.

On other platforms, use a destination directory with suitable access controls.

## History-free restores and older artifacts

Server 0.0.17 retains the 0.0.14 fix that preserves live resource revisions and creates current temporal
snapshots when restoring a backup made with `--include-history false`. Default
history-inclusive backups taken afterward remain restorable, including after
further updates and deletions. Earlier history omitted from the artifact remains
absent; the snapshots start a new timeline at the restore boundary. Prefer
history-inclusive backups when the earlier history is needed for recovery.

Existing history-free format 5 artifacts can be restored directly with the
matching 0.0.14 executor. Upgrading binaries alone does not recreate history
missing from a database previously restored by 0.0.13. Restore a valid artifact
using the fixed executor to establish a consistent state before relying on new
backups. Backup creation now rejects inconsistent snapshots instead of producing
an artifact that fails staging. Do not edit revision or history fields to bypass
validation.

The earlier 0.0.13 error, `Full backup live revisions disagree with
'collection_history'`, is covered by a regression check that now requires
successful staging and a complete second-generation restore on 0.0.17.

## Reproduce the integration check

```sh
cargo build --locked
python3 scripts/test-backup-restore.py
```

Python 3.9+ and Docker or Podman are required. Use `--runtime docker` or
`--runtime podman` to select the runtime. The script creates its own disposable
database, pins the server and PostgreSQL images, applies migrations, and starts
the restore executor. It first checks chat webhook presets, real server previews and pacing updates,
structured search, streaming JSONL, fresh
credential approvals, task discovery, and schema evolution. It exercises backups
with and without history, both
confirmation modes, receipt-only status after token invalidation, and recovery
of a deleted object with its original revision and timestamps after password
reset. It also stages a default backup immediately after a history-free restore,
then takes and fully restores another default backup after further updates and
a deletion. Its containers and network are removed
afterward. It accepts no external server URL.
