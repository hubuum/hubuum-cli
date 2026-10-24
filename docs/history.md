# History and historical views

CLI v0.0.20 uses client 0.19.0 against Hubuum server v0.0.20.
History describes retained snapshots. It does not promise a complete event ledger
or reconstruct records that the server no longer retains.

## Select an identity or a historical name

Existing `history class`, `history object`, and `history show` commands accept
`--class NAME` or `--class-id ID`. Object commands also accept `--name NAME` or
`--object-id ID`. Names resolve against live resources; numeric IDs bypass that
lookup and allow retained history to be queried after deletion. Supply exactly
one class selector and at most one object selector.

```sh
hubuum-cli history class --class Hosts
hubuum-cli history object --class-id 10 --object-id 42 --all
hubuum-cli history show --class-id 10 --object-id 42 --id 90001
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-10-23T12:00:00Z
```

History IDs use a direct entry endpoint. Listing tables show history ID, resource
ID, name, revision, operation, and validity interval. JSON for these existing
commands keeps the complete snapshot, including object data and class schemas.

Use `object-name` to search exact names in history across different object IDs,
including renamed, deleted, or recreated objects. A numeric name stays a name:

```sh
hubuum-cli history object-name --class-id 10 --name 123 --all
hubuum-cli history object-name --class-id 10 --name old-host --revision 2 --sort -history_id
```

## Navigate timelines

Timelines return compact change markers. Fetch a complete snapshot with its
history ID when needed.

```sh
hubuum-cli history timeline class --class-id 10 --direction older --limit 20
hubuum-cli history timeline object --class-id 10 --object-id 42 --anchor 90001 --direction newer
hubuum-cli history timeline object --class Hosts --name web-01 --from 2026-10-01T00:00:00Z --until 2026-11-01T00:00:00Z
```

`--from` is inclusive and `--until` is exclusive. `--at` seeks a timestamp;
`--anchor` seeks past a history ID without returning that entry. These two seek
options are mutually exclusive. Direction defaults to `older`.

## Read an as-of view

```sh
hubuum-cli history objects --class-id 10 --at 2026-10-23T12:00:00Z --all
hubuum-cli history relations --class-id 10 --all
hubuum-cli history relations --class-id 10 --object-id 42 --at 2026-10-23T12:00:00Z
hubuum-cli history computed --class-id 10 --object-id 42 --at 2026-10-23T12:00:00Z
```

`objects` returns the retained class snapshot/schema and its object population.
`relations` returns direct relationship changes, or the direct relationships at
`--at`. Each endpoint contains historical context or an explicit unavailable
reason; current names are never substituted for missing history.

`computed` recomputes shared definitions using retained historical inputs. Its
response preserves availability reasons and definition revisions. Personal
computed fields are explicitly unsupported.

## Pagination and pipelines

The new paged views accept `--limit` (up to 250), `--cursor`, `--include-total`,
and `--all`. Totals are opt-in. `--all` follows cursors with a limit of 1,000 pages
or 100,000 records and rejects repeated cursors or changing envelope context.
For text output, use `next` in the REPL to continue the same query.

New views return an envelope containing `items`, `objects`, or `relations`, plus
`next_cursor`, `total_count`, `page_limit`, and `returned_count`. Population and
relationship responses also retain their coverage and as-of context. Pipelines
receive this complete envelope:

```sh
hubuum-cli history objects --class-id 10 --at 2026-10-23T12:00:00Z --all --output json | JQ '{coverage, class, objects}'
```

## Server v0.0.20 deletion ordering

Wait for class reindex or rebuild tasks to finish before deleting that class.
Concurrent deletion and rebuild work can deadlock on server v0.0.20. Use
`task list` to find pending work and `task wait ID` before `class delete`.
