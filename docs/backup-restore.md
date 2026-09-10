# Backup and restore

CLI v0.0.11 uses `hubuum_client` 0.10.1 and targets Hubuum server 0.0.14.
Backups and restore staging/confirmation require administrator access.

## Prepare the server

Upgrade the server, `hubuum-admin`, and template-worker binaries together to
0.0.14. Run `hubuum-admin --migrate` before starting the server when upgrading
from an older schema; v0.0.14 adds no migration over v0.0.13. Deploy a matching
`hubuum-admin --restore-executor` process against the same database; it performs
queued restores. Server 0.0.14 includes the drain coordination and JSON-null
insertion fixes from 0.0.13 and fixes subsequent backups after history-free
restores. Update any separately deployed restore executor as well as the server.

This target uses backup format 5. Restore format 4 artifacts with a compatible
older server, then upgrade and take a new backup. Changing `backup_version` in a
file does not migrate its contents.

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

Format 5 excludes password hashes, bearer tokens, and token scopes. Its manifest
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
format 5 backups. Recheck integration configuration before resuming automation.

## File handling

Backup and receipt files use owner-only permissions (`0600`) on Unix. Existing
files require `--force`. Directories, symbolic links, and devices are rejected;
provide a regular file path. Writes use a temporary file in the destination
directory followed by atomic installation. Failed remote operations leave an
existing destination intact. If installation fails after the content is written,
the error gives the private temporary file path for recovery.

On other platforms, use a destination directory with suitable access controls.

## History-free restores and older artifacts

Server 0.0.14 preserves live resource revisions and creates current temporal
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
successful staging and a complete second-generation restore on 0.0.14.

## Reproduce the integration check

```sh
cargo build --locked
python3 scripts/test-backup-restore.py
```

Python 3.9+ and Docker or Podman are required. Use `--runtime docker` or
`--runtime podman` to select the runtime. The script creates its own disposable
database, pins the server and PostgreSQL images, applies migrations, and starts
the restore executor. It exercises backups with and without history, both
confirmation modes, receipt-only status after token invalidation, and recovery
of a deleted object with its original revision and timestamps after password
reset. It also stages a default backup immediately after a history-free restore,
then takes and fully restores another default backup after further updates and
a deletion. Its containers and network are removed
afterward. It accepts no external server URL.
