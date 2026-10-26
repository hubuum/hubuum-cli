# History and historical views

CLI v0.0.21 uses client 0.19.0 against Hubuum server v0.0.20.
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
ID, name, revision, operation, and validity interval. Field changes appear
below the table. Comparisons use the preceding revision of the same resource when it is present in the
results, regardless of display order. Nested changes use JSON Pointer paths,
with `+` for additions, `-` for removals, and `before → after` for replacements.
When a changes block contains additions or removals, replacement lines receive
two leading spaces so the root `/` paths align. Blocks containing only
replacements have no extra indentation.
Creation and deletion are labeled explicitly. A missing preceding revision is
reported as unavailable; use `--all` to compare across page boundaries. JSON
keeps the complete snapshot, including object data and class schemas.

## Flexible times and completion

For `history show`, `history class`, and `history object`, a shortened `--at`
selects **all changes whose `valid_from` falls in that UTC interval**. The start
is inclusive and the end is exclusive. Supported precision runs from year to
second:

```sh
hubuum-cli history show --class-id 10 --object-id 42 --at 2026
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-01
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-01-01
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-01-01T15
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-01-01T15:00
hubuum-cli history show --class-id 10 --object-id 42 --at 2026-01-01T15:00:00
```

These queries follow every timeline page and retrieve the matching snapshots,
including deletion records. Text shows a history table; JSON returns a list even
when only one record matches, or an empty list when no changes match. Existing
`--include-total` behavior wraps that list with pagination metadata. Date windows
use the same 1,000-page / 100,000-record bounds as other complete history views.
Do not combine `--at` with `--limit`, `--sort`, `--cursor`, or `--all`.

A full RFC3339 timestamp with a timezone retains its original **as-of** meaning:
it returns the snapshot valid at that instant. Subseconds and timezone offsets
are supported. Appending `Z` to `2026-01-01T15:00:00` therefore selects the state
at exactly that instant instead of changes during that second.

Relative times also select an as-of instant in the past:

```sh
hubuum-cli history show --class-id 10 --object-id 42 --at 1mo1w4d3h44m1s
hubuum-cli history show --class-id 10 --object-id 42 --at -3h
hubuum-cli history show --class-id 10 --object-id 42 --at now
```

Use `mo` for calendar months, `w` for weeks, `d` for days, `h` for hours, `m` for
minutes, and `s` for seconds, in that order. Each part is optional, but supply at
least one part. A leading `-` is optional; both forms mean time ago. Calendar
months are subtracted first, clamping to the last day of the target month when
needed. The remaining units are fixed durations; a day is 24 hours.

Other history commands (`timeline`, `objects`, `relations`, and `computed`)
accept the same time syntax for their instant parameters. A shortened date uses
the **start** of its UTC interval for these views. Timeline `--from` and `--until`
also accept shortened and relative times.

In the REPL, provide `--class` or `--class-id` and, for an object, `--name` or
`--object-id`, then press Tab after `--at`. Completion fetches retained timeline
timestamps and offers years with changes. Tab again offers observed months,
then days, hours, minutes, seconds, and finally exact timestamps. Intermediate
completions leave the cursor on the value so another Tab can refine it. Results
are cached per resource for 30 seconds and cleared after mutations; API
completion respects `completion.disable_api_related`.

Use `object-name` to search exact names in history across different object IDs,
including renamed, deleted, or recreated objects. A numeric name stays a name:

```sh
hubuum-cli history object-name --class-id 10 --name 123 --all
hubuum-cli history object-name --class-id 10 --name old-host --revision 2 --sort -history_id
```

## Navigate timelines

Timelines show a compact table with history ID, revision, the UTC change time,
and a `changes` column. The date and time share a cell on separate lines to leave
room for the differences. Fetch a complete snapshot with its history ID when
needed.

```sh
hubuum-cli history timeline class --class-id 10 --direction older --limit 20
hubuum-cli history timeline object --class-id 10 --object-id 42 --anchor 90001 --direction newer
hubuum-cli history timeline object --class Hosts --name web-01 --from 2026-10-01T00:00:00Z --until 2026-11-01T00:00:00Z
hubuum-cli history timeline object --class Hosts --name escher.uio.no --against-latest
```

`--from` is inclusive and `--until` is exclusive. `--at` seeks a timestamp;
`--anchor` seeks past a history ID without returning that entry. These two seek
options are mutually exclusive. Direction defaults to `older`.

By default, each entry is compared with the preceding **retained entry** in
chronological order, even when that entry falls outside the selected page or
time window. The comparison remains `previous → entry` when navigating in either
direction. A retention or authorization gap can mean that this is not the
immediately preceding revision; the JSON reference ID identifies the actual
comparison baseline.

`--against-latest` compares every displayed entry with the newest retained
snapshot, independently of the selected window, anchor, or cursor. Its arrows
mean `entry → latest`. The newest entry itself is labeled `Latest retained
entry`. Creation, deletion, restoration, unchanged fields, and unavailable
snapshots are distinguished explicitly.

The CLI retrieves full snapshots in batches of up to 250 from the history-list
endpoint, with one separate boundary-reference lookup. Previous-entry comparisons
skip that lookup when the oldest displayed entry is the initial creation
(revision 1). Scanning stops when all required snapshots are found; the CLI does
not request each snapshot separately.
Older windows can require several snapshot pages because the snapshot-list API
does not expose timeline windows or timeline cursors through the client. Scans
are bounded to 1,000 pages or 100,000 snapshots and reject repeated cursors.
Shortened-date matches in `history show`, `history class`, and `history object`
also use these batched snapshot reads.

JSON and pipelines preserve the timeline envelope, pagination metadata, and all
marker fields. Each item additionally contains a `changes` object with `mode`
(`previous` or `latest`), `against_history_id` (or `null` when no reference entry
was found), and `summary`:

```sh
hubuum-cli history timeline object --class Hosts --name escher.uio.no --against-latest --output json | JQ '.items[] | {history_id, changes}'
```

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
Server v0.0.21 fixes concurrent deletion/rebuild deadlocks. When using the older
v0.0.20 server, use
`task list` to find pending work and `task wait ID` before `class delete`.
