# Task discovery

Server v0.0.16 retains task targets, submitted options, and output availability.
The CLI exposes this data in `task show` and uses typed discovery filters in
`task list`. Absent metadata on older tasks stays absent; false option values are
preserved.

```sh
hubuum-cli task list --kind export,backup --status succeeded,failed --where terminal equals true --all
hubuum-cli task list --kind backup --where backup_include_history equals false
hubuum-cli task list --kind schema_validation --where class_id equals 42 --where schema_revision equals 3
hubuum-cli task list --where created_after equals 2026-09-01T00:00:00Z --where output_state equals available
hubuum-cli task show 123 --output json
```

Kinds are `export`, `import`, `backup`, `reindex`, `remote_call`, and
`schema_validation`. Statuses include `queued`, `validating`, `running`, `succeeded`,
`partially_succeeded`, `failed`, and `cancelled`. Kind and status accept
comma-separated sets; existing CLI aliases continue to work.

Each `--where` uses `FIELD equals VALUE`, and fields cannot be repeated.
Timestamp bounds use the named `_after` and `_before` fields and RFC3339 values.
Boolean values are `true` or `false`; IDs and revisions must be positive.
The client validates combinations of task kinds and specialized filters before
sending. Pagination preserves all typed filters. REPL Tab completion offers the
supported filter fields and values where available.

| Area | Discovery fields |
| --- | --- |
| Lifecycle | `terminal`, `cancel_requested`, `terminal_reason`, `output_state` |
| Attribution | `trace_id`, `submitted_by` |
| Time | `created_after`, `created_before`, `started_after`, `started_before`, `finished_after`, `finished_before` |
| Targets | `class_id`, `object_id`, `collection_id`, `class_relation`, `object_relation` |
| Schema/rebuild | `schema_revision`, `computation_revision`, `schema_work_kind`, `schema_work_status` |
| Remote | `remote_target_id`, `remote_side_effect_state` |
| Export | `export_scope_kind`, `export_template_id`, `export_has_warnings`, `export_truncated` |
| Import | `import_dry_run`, `import_atomicity`, `import_collision_policy`, `import_permission_policy`, `import_has_failed_items` |
| Backup | `backup_include_history` |

`task show` includes retained details, including target identity and submitted
options, for supported task kinds. Output availability distinguishes retention
state from task success; an older successful task may no longer have downloadable
output. Discovery does not add generic output-download commands: use the existing
export, import, backup, and schema commands for their respective artifacts.
