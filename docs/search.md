# Search

Search can match plain text across collections, classes, and objects, or evaluate
structured predicates on one resource kind. Structured search uses server
v0.0.16's version 1 `POST /api/v1/search` API. Local output pipelines run afterward
on the resources returned by that search.

## Terminal predicates

```sh
hubuum-cli search --target object --class Hosts --where 'data.cpu.cores >= 8 AND (name ~ "^srv-" OR data.retired IS NULL)' --sort name asc --limit 25
hubuum-cli search --target user --where 'name IN ["alice", "bob"] AND email IS NOT NULL' --include-total
```

Targets are `object`, `class`, `collection`, `audit_event`, `user`, `group`, and
`service_account`. `--class` restricts an object search to an exact class name.
The server applies the caller's permissions to results and counts.

The predicate is one quoted argument. In the REPL, start with
`search --target object --class Hosts --where '` and press Tab inside the open
quote. Completion offers target fields, class data paths, operators, boolean
continuations, and parentheses. Data-path suggestions use the existing bounded
field discovery/cache and honor the API-completion setting. Use double quotes
for string literals inside the outer single quotes, then close the outer quote
before execution. Tab completion here is provided by the interactive Hubuum REPL;
external shell completion is unchanged.

| Syntax | Meaning |
| --- | --- |
| `name == "srv-01"`, `name != "srv-01"` | Equality or negated equality |
| `data.cpu.cores >= 8` | Numeric comparison; also `<`, `<=`, `>` |
| `name ~ "^srv-"`, `name !~ "test"` | Regular expression or its negation |
| `name IN ["a", "b"]` | Membership; `NOT IN` is also supported |
| `data.retired IS NULL` | Null test; `IS NOT NULL` is also supported |
| `NOT (...)`, `... AND ...`, `... OR ...` | Boolean expressions; precedence is NOT, AND, OR |

Use JSON numbers and `true`/`false` for typed values. `data.*` refers to the
server's `json_data` field. JSON paths use dot-separated nonempty ASCII letters,
digits, `_`, or `$`. Operators must be supported by the selected field; numeric
ordering on names, for example, is rejected before sending.

This is the server-search subset of the [local predicate grammar](DSL.md).
`AS` casts, array fanout/index selectors, computed selectors, and `IS MISSING`
are not supported by this endpoint. Server null tests can include absent JSON
paths; negation follows the server's authorized-result set semantics. Use a
local pipeline when you need local missing/null distinctions or casts. Relation
predicates and specialized network/JSON operators are available through query
files.

## Query files

Save a version 1 request as `search.json`:

```json
{
  "version": 1,
  "target": { "kind": "object", "class": { "name": "Hosts" } },
  "filter": {
    "op": "and",
    "args": [
      {
        "op": "field",
        "predicate": {
          "field": "json_data",
          "path": "cpu.cores",
          "operator": "gte",
          "value": 8
        }
      },
      {
        "op": "related",
        "predicate": {
          "class": { "name": "Rooms" },
          "depth": 1,
          "filters": [
            { "field": "name", "operator": "equals", "value": "B-101" }
          ]
        }
      }
    ]
  },
  "sort": [{ "field": "name", "direction": "asc" }],
  "limit": 25,
  "include_total": true
}
```

```sh
hubuum-cli search --query-file search.json --output json
hubuum-cli search --query-file search.json --all \| P id name data.cpu.cores
```

Class selectors accept either `name` or a positive `id`. A related predicate is
valid only for object searches. Its filters apply to reachable objects of the
selected class. `not` wraps one `arg`; `and` and `or` require at least two `args`.

Validation rejects unknown fields, unsupported versions, invalid selectors,
operators, and sorts before sending. Requests are bounded to 64 KiB, expression
depth 8, 64 expression nodes, 32 field predicates, four related predicates,
16 filters per relation, and relation depth 1–10. Arrays contain 1–50 non-null
scalars; nested values, empty strings, and commas inside array strings are
rejected. The server performs remaining semantic checks and enforces its query
budgets. The terminal DSL also obeys these bounds.

## Pagination and output

Use repeated `--sort FIELD asc|desc` in terminal queries, with at most eight
unique sort fields. `--limit`, `--cursor`, and `--include-total` can override
pagination settings in either mode. `--query-file` cannot be combined with
`--target`, `--class`, `--where`, or `--sort`.

The REPL remembers the next request for `next` or Enter according to configuration.
Keep filters, target, and sort unchanged when reusing a cursor. `--all` follows
cursors before applying a pipeline, rejects cursor loops, and stops at a limit
of 10,000 pages or one million records. Without `--all`, a pipeline warns when
it has only the current page. All-page output uses memory proportional to the
returned resources.

Structured `--output json` preserves the response envelope:
`version`, `kind`, `results` entries with `kind` and `resource`, `next`, and
optional `total`. Text, JSONL, and semantic pipelines operate on resource rows
with original lower-case fields such as `id`, `name`, and `data`. Text includes
next-page and requested-total hints. Plain-text search options, including
`--stream`, cannot be mixed with structured mode.

## Incremental plain-text search

```sh
hubuum-cli search server --kind class --kind object --limit-per-kind 5
hubuum-cli search server --stream --output jsonl
```

Unredirected text batches and JSONL event envelopes are flushed as they arrive.
Text batches include any next-page cursor; reuse it with `--cursor-collections`,
`--cursor-classes`, or `--cursor-objects` for the corresponding result kind.
JSONL contains one serialized `started`, `batch`, or `done` event per line,
identified by `event`, with its payload in `data`. Batch payloads contain
`kind`, `collections`, `classes`, `objects`, and `next`.

`--output json` retains the complete event array. Local pipelines, extension
captures, and application file redirects buffer results so transformations see
a complete page and failed redirects preserve the previous file. A shell's own
redirection simply captures stdout as usual. `--all` and `--stream` are mutually
exclusive. Server error events and a connection ending before `done` exit with
an error; already emitted terminal batches remain visible and are not replayed.
