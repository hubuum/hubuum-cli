# Schema evolution and task cancellation

The CLI targets Hubuum server v0.0.16 through `hubuum_client` 0.12.0.
The schema lifecycle below was introduced in server v0.0.15.
All schema policy writes use `class schema`. The old `--schema`/`-s` and
`--validate`/`-v` flags on `class create` and `class modify` are removed.
Create the class first, then use the same staged workflow for initial setup and
later changes. Commands accept `--class NAME` or the class name as a positional.

Default text output summarizes policies, compliance pages, and schema work.
Impact summaries show readiness, progress, transition counts, and up to five
failure groups with sample object IDs. Aligned labels use `output.padding`, with
indented results, impact, and failure sections. Zero-only secondary counters are
omitted from text. Large schemas and per-object diagnostics
are omitted from text. Use `--output json` for the complete response, semantic
pipelines to select diagnostic fields, or HTML repair reports for detailed review.

In the REPL, Tab completes `--task` for `work`, `cancel`, `report`, and
`generate-report`, and `--impact-task` for activation. Suggestions show the latest
50 visible schema tasks with status/summary labels, refreshed after five seconds
or a local mutation. The server cannot filter the task list by class, so suggestions
can include other classes; the work command still validates class ownership.
Completion respects `completion.disable_api_related` and does not fetch findings.

## Stage, inspect, activate

```sh
hubuum-cli class create --name Hosts --collection main --description hosts
hubuum-cli class schema show Hosts --output json
hubuum-cli class schema stage Hosts --schema '{"type":"object"}' --validate true
hubuum-cli class schema revisions Hosts --limit 50
hubuum-cli class schema revision Hosts --revision 2
hubuum-cli class schema impact Hosts --revision 2
hubuum-cli class schema work Hosts --task 123 --output json
hubuum-cli class schema activate Hosts --revision 2 --expected-active-revision 1 --impact-task 123
```

Replace example revisions and task IDs with returned values. Staging requires
an explicit validation boolean unless copying a previous revision. Omit `--schema` to copy the active schema and
change only its validation setting. `--schema` accepts
inline JSON, a `file://` local JSON file, or an HTTP(S) URL. To remove the schema, stage
`--schema null --validate false`, then analyze and activate that revision.
For example, preview enabling validation against the existing schema:

```sh
hubuum-cli class schema stage Hosts --validate true
hubuum-cli class schema impact Hosts --revision 2
hubuum-cli class schema work Hosts --task 123
```

Use the returned revision and task IDs. The active policy stays unchanged until
explicit activation. Use `--validate false` to stage disabling validation while
keeping the schema. If no active schema exists, enabling validation requires
`--schema`. `class schema help` shows a short version of this workflow.

Impact and revalidation return immediately with a task ID. Poll `class schema
work` until `status` is `complete`, inspect `readiness` and findings, then activate.
Schema revisions are distinct from resource revisions. Activation compares the
explicit expected active revision and current object population. On a conflict,
refresh state and impact analysis before deliberately retrying.

The default activation policy rejects incompatible objects. `--allow-pending`
explicitly permits existing pending/invalid objects and requires an unscoped
administrator; new writes still obey the activated policy. Inspect `task_id` and
`dependent_rebuild_task_id` in the response for follow-up work.

```sh
hubuum-cli class schema abandon Hosts --revision 3
hubuum-cli class schema revalidate Hosts --revision 2
hubuum-cli class schema objects Hosts --status pending --limit 50
hubuum-cli class schema objects Hosts --status pending --limit 50 --after 100
```

Compliance statuses are `valid`, `invalid`, `pending`, and `not_required`.
Pages have limits of 1–100 (default 50). Follow compliance `next_after` even on
empty visible pages; authorization filtering can hide scanned objects. Revision
lists resume with `--after` set to the last returned revision. These commands
return one page per invocation. Administrative state and diagnostics require an
unscoped administrator; compliance results respect object visibility.

## Return to a previous policy

```sh
hubuum-cli class schema revisions Hosts
hubuum-cli class schema stage Hosts --from-revision 4
hubuum-cli class schema impact Hosts --revision 8
hubuum-cli class schema work Hosts --task 125
hubuum-cli class schema activate Hosts --revision 8 --expected-active-revision 7 --impact-task 125
```

Copying creates a new staged revision with the previous schema and validation
setting; it does not reactivate or modify the historical revision. Use returned
IDs and the current active revision instead of the example numbers. Analyze the
impact against today's objects before explicitly activating the new proposal.
Add `--validate true` or `--validate false` to override the copied setting.
`--from-revision` and `--schema` cannot be combined. This restores schema policy,
not past object data.

## Repair reports

```sh
hubuum-cli class schema generate-report Hosts --task 123 --object-url-template 'https://inventory.example/objects/{object_id}' > repair.html
hubuum-cli class schema report Hosts --task 123 > retained-repair.html
```

Generation uses saved findings and retains the HTML. Retrieval does not rerun
validation. `--template-id` selects a stored HTML layout when generating.
Text output emits HTML; `--output json` returns a JSON string. Server report
budgets and the configured client response-size limit apply. Findings include
object revisions, JSON Pointer locations, constraints, and explicit diagnostic
omissions; actual scalar values are redacted.

## Task cancellation

```sh
hubuum-cli task cancel 123 --reason 'Withdraw queued work' --expected-status queued
hubuum-cli task show 123
hubuum-cli task list --kind schema_validation
hubuum-cli class schema cancel Hosts --task 123
```

`--expected-status` is optional. Use `queued` when running work must not be
cancelled; a conflict means the status changed. Reasons must be nonblank,
single-line, and at most 512 UTF-8 bytes. Cancellation records durable intent:
a returned running status means executor cleanup is still in progress. Poll
`task show` until terminal. Repeated cancellation is idempotent; completed work
is not undone. Inspect cancellation timestamps, execution deadline, terminal
reason, unattempted items, and remote side-effect state. `PossiblySent` and
`LegacyUnknown` do not establish that a remote effect was prevented.

Owners and administrators can cancel authorized tasks. Scoped tokens can cancel
only work submitted with the same token. Internal schema/reindex work requires
an unscoped administrator. Local background-job forgetting does not cancel a
server task.

## Imports and server migration

`import submit` accepts the client's `schema_activation` class input, containing
`revision`, `expected_active_revision`, `policy` (`reject_incompatible` or
`allow_pending`), and optional `impact_task_id`. Include the exact staged
`json_schema` and `validate_schema` in the class input. Activation and import are
atomic. Legacy overwrite imports that change populated-class policy return 409.

Drain old workers, schedule a quiet migration window, run `hubuum-admin
--migrate`, and install matching server, administrator, template-worker, and
restore-executor binaries. Revalidate existing enforced classes after migration.
External authorization must grant `CancelTask`. Restart string-sorted pagination
following the server upgrade. Review stored schemas against the server's stricter
reference, complexity, data, and validation budgets before resuming writes.
Backups now use format 6; see [backup migration](backup-restore.md).

See the [server v0.0.15 release notes](https://github.com/hubuum/hubuum/releases/tag/v0.0.15)
and [schema validation limits](https://github.com/hubuum/hubuum/blob/v0.0.15/docs/json_schema_validation.md).
