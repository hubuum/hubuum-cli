# Object data and computed fields

The Hosts, Rooms, and other specialist resources below are illustrative fixtures.
For an immediately loadable inventory, use [Atlas](example-dataset.md).

Atomically patch an object's raw data through exact class and object names. The
patch can be inline, loaded from `@FILE`, or loaded through the existing
`file://FILE` value-source form:

```sh
hubuum-cli --hostname api.example.com --token-file /run/secrets/hubuum.token \
  object data patch --class Hosts --name srv-01 \
  --patch @facts-patch.json --create --description "Managed by Ansible"
```

With `--create`, Hubuum CLI initializes a missing object by applying the patch to
an empty JSON object. A concurrent create conflict causes one exact-name PATCH
retry. In this example, RFC 6902 `add` at `/facts` creates or completely replaces
that member without changing other object data. The path and its contents are
chosen by the consumer. See the
[Ansible fact publication guide](ansible-facts.md) for the accepted JSON
Patch format, create-if-missing behavior, and service-account permissions.

Use `object show --path` to select data with JSONPath. The path starts at the
object's data: `$` is its root, `.field` selects a member, `[0]` selects the first
array element, and `[*]` selects every array element. Quote the expression so your
shell preserves `$` and `*`. If the root is missing, the CLI tries adding `$` or
`$.` and warns when that produces a valid path: `.facts` and `facts` become `$.facts`.

```sh
hubuum-cli object show --class Hosts --name host-1 --path '$.facts'
hubuum-cli object show --class Hosts --name host-1 --path '$.facts.operating_system'
hubuum-cli object show --class Hosts --name host-1 --path '$.facts.network.interfaces[0].name'
hubuum-cli object show --class Hosts --name host-1 --path '$.facts.network.interfaces[*].name'
hubuum-cli help jsonpath
```

`--path` implies `--data` and keeps the usual flattened key/value rows, with full
field paths. It only filters the text data display; `--json` or `--output json`
returns the full object as JSON. Run `help jsonpath` in the REPL for the same
JSONPath guidance and examples.

Different selected values that flatten to the same key keep separate rows.
Selections with no flattened fields, such as empty objects or arrays containing
only empty objects, remain visible as their original values.

Related graphs support a positive `--limit` on `class show`, `object show`,
`relation class graph`, and `relation object graph`:

```sh
hubuum-cli object show --class Hosts host-1 --max-depth 2 --limit 1000
hubuum-cli relation class graph --root-class Hosts --limit 1000
```

This is a server-side graph size guard for related entries, excluding the root.
A graph that exceeds the limit returns an
error instead of partial results; it is not a page size or output truncation.
Omitting `--limit` keeps the server's default. Zero is rejected by the CLI.
Use `--max-depth` to narrow traversal when a graph is too large.

## Filter lists by related objects

CLI v0.0.20 can restrict `object list` to objects connected to matching
objects in another class. Filtering happens on the server before pagination:

```sh
hubuum-cli object list --class Hosts --related room=Rooms --related-depth room=2 --related-where room.name equals B701
hubuum-cli object list --class Hosts --related room=Rooms --related-depth room=2 --related-where room.data.floor gte 3 --computed all --all
hubuum-cli object list --class Hosts --related-id rack=42 --related-where rack.name startswith rack- --include-total
```

`--related ALIAS=CLASS` selects an exact class name. Quote the whole assignment
for names containing spaces, such as `--related 'room=Server Rooms'`. Numeric
class names stay names; use `--related-id ALIAS=ID` for positive IDs. The options
may appear in any order. A group without `--related-where` requires any reachable
object in its target class.

Repeat `--related-where ALIAS.FIELD OP VALUE` to require every predicate to match
**the same related object**. Repeat `--related` with distinct aliases for separate
existence tests, combined with AND; those tests may match different targets.
Traversal follows either relationship direction, excludes the root object itself,
and respects server permissions. Depth defaults to 1 (direct neighbors); each
`--related-depth ALIAS=DEPTH` sets one group's maximum distance from 1 through 10.
Negated field operators still require a related object; `not_equals` does not mean
"has no relationship". Use [structured search](search.md#query-files) for that,
OR expressions, or negation of a whole related predicate.

Fields are `id`, `name`, `description`, `collection_id`, `revision`, `created_at`,
`updated_at`, and dotted `data.*` / `json_data.*` paths. They use the same operator
and value grammar as `--where`. Related computed fields and JSON array selectors
are unsupported. Related fields filter the targets; displayed rows and pipeline
fields remain the source objects.

At most four groups are allowed. Aliases contain 1–64 ASCII letters, digits, or
underscores and cannot start with a digit. Duplicate aliases, repeated depths,
undeclared aliases, invalid class IDs, depths, fields, operators, and JSON paths
fail before resource lookup. Server query budgets still apply.

These options combine with ordinary `--where`, `--sort`, `--computed`, `--limit`,
`--cursor`, `--include-total`, `--all`, and output pipelines. Keep all groups and
predicates unchanged when reusing a cursor; the REPL's next-page command preserves
them. Computed-field sorting still fetches and sorts all matching objects locally
and rejects a server cursor. REPL completion offers class names after `ALIAS=`,
declared aliases and target fields for `--related-where`, and supported operators.

## Hydrate paths to a target class

```sh
hubuum-cli object hydrate --class Hosts --name web-01 --target-class Rooms --max-depth 2 --path-limit 20
hubuum-cli object hydrate --class-id 10 --object-id 42 --target-class-id 20 --output json
```

Supply exactly one name or ID for each class selector and for the root object.
Depth accepts 1–10 and defaults to 1; the path limit accepts 1–50 and defaults to
20. The result retains the root, target, objects, relations, paths, and
`truncated` indication, including in pipelines. Hydration has no continuation
cursor. A truncated response requires a narrower request or a larger bounded
path limit.

## Directional relation labels

```sh
hubuum-cli relation class create --class-a Hosts --class-b Rooms --forward-label 'located in' --reverse-label contains
```

The forward label describes class A toward class B; the reverse label describes
the opposite direction. Labels, template aliases, and cardinality settings stay
with their respective endpoints when the CLI orders class IDs for the API.

Administrators can inspect the server's redacted effective process configuration:

```sh
hubuum-cli admin config
hubuum-cli admin config --output json
```

Fetch Prometheus exposition text without logging in. The default route is `/metrics`;
use the path reported by `admin config` when the server has configured another route:

```sh
hubuum-cli metrics
hubuum-cli metrics --path /internal/metrics
```

Computed fields can be managed as shared class definitions or personal
definitions. Paths are JSON Pointers into object `data`:

```sh
hubuum-cli computed shared create --class Hosts --key average_load --label "Average load" --operation average --path /load/one --path /load/five --result-type number
hubuum-cli computed shared list --class Hosts
hubuum-cli computed personal list --class Hosts
hubuum-cli object show --class Hosts host-1 --computed S:average_load
hubuum-cli object list --class Hosts --computed all --output json
```

In the REPL, data-field completion merges the selected class's JSON Schema with
a sample of up to 100 objects, using the same depth-six traversal as
`class fields`. This supplies escaped JSON Pointers for computed `--path`
options and dotted paths for aggregate dimensions, measures, and filters.
Inspected fields are cached for `cache.time` seconds (one hour by default) and
the cache can be bypassed with `cache.disable`.

`class fields --name <class>` is also the field inventory for downstream
selectors. Alongside sampled `data.*` paths, it lists enabled shared and
personal computed fields as `S:<key>` and `P:<key>`. The `Source` column
distinguishes the three kinds; counts, types, and examples are observed from the
same object sample, so a computed definition with no sampled value still
appears with an empty observation. The former `object fields --class <class>`
spelling remains available as a deprecated compatibility alias and prints an
exact replacement command when invoked.

Without per-class configuration, computed values are off by default. Use repeatable, dynamically completed
`--computed S:<key>` and `--computed P:<key>` options to select individual
shared or personal fields, or `--computed all` to select every field:

```sh
hubuum-cli object list --class Hosts --computed S:average_load --computed P:preferred_name
hubuum-cli object show --class Hosts host-1 --computed all
```

Per-class defaults apply to both object list and show commands:

```toml
[output.object_class_computed_fields]
Hosts = ["S:average_load", "P:preferred_name"]
Switches = ["all"]
```

They can also be changed from the CLI; the key and value both support dynamic
completion:

```sh
hubuum-cli config set --key output.object_class_computed_fields.Hosts --value S:average_load,P:preferred_name
hubuum-cli config unset --key output.object_class_computed_fields.Hosts
```

An explicit `--computed` selection replaces the class default for that command.
Use `--computed none` to suppress configured defaults temporarily.

Object-list text output renders selected values as compact scoped columns.
Selected JSON output retains scope metadata such as revisions while excluding
unselected values; `--computed all` retains the complete computed envelope.
Computed columns can also be sorted with the same scoped names:

```sh
hubuum-cli object list --class Hosts --sort S:average_load desc --limit 10
hubuum-cli object list --class Hosts --sort P:preferred_name asc
```

The CLI fetches all matching objects for computed sorting, sorts them locally,
and then applies `--limit`. Computed sorting cannot
be combined with `--cursor`. A computed sort fetches its key internally but does
not display it unless the same field is selected with `--computed`.

Related objects can be selected by target class name without specifying any
intermediate classes:

```sh
hubuum-cli relation object list --root-class Person --root-object Alice \
  --where class equals Hosts --max-depth 10 --all
```

This includes paths such as Person → Room → Host and other connecting paths,
subject to server limits and permissions. The default maximum depth is 2;
`--all` follows pagination, while `--max-depth` bounds traversal distance.
Related class and object queries also accept `--where collection equals Inventory`.
Class and collection filter values support name completion.

To fetch a neighborhood with object data and all connecting edges, use JSON graph
output with `--data`:

```sh
hubuum-cli relation object graph --root-class Rooms --root-object B701 \
  --max-depth 2 --data --output json
```

Each entry in `objects` then includes `data` (including explicit `null` values),
reusing the graph response without further object lookups. Without `--data`, the
graph keeps its existing metadata-only JSON shape. Use `relations` to find every
connection; each object's `path` describes only one shortest path. Graph size
limits return errors instead of partial results.

Object-list text and pipeline output automatically promotes dotted data fields
referenced by `--where` into explicit columns. This makes the matching value
visible without separately repeating the path in `--data-columns`:

```sh
hubuum-cli object list --class Hosts \
  --where json_data.facts.operating_system.major_version lt 8
hubuum-cli object list --class Hosts \
  --where data.environment equals production \
  --include-where-results false
```

The second form keeps the normal configured or automatic data-column layout.
Raw JSON output already contains these values in the nested `data` object and
is not flattened.

Run permission-scoped aggregation on the server with `object aggregate`.
`--group-by` accepts scalar object fields, dotted `data` paths, and computed
selectors. Numeric measures use `operation:field`; repeat dimensions up to three
times and measures up to four times:

```sh
hubuum-cli object aggregate --class Hosts --group-by data.os_version
hubuum-cli object aggregate --class Hosts \
  --group-by data.region \
  --aggregate sum:data.cpu.cores \
  --aggregate average:S:load \
  --sort object_count desc \
  --limit 25 --include-total
hubuum-cli object aggregate --class Hosts \
  --aggregate average:data.cpu.cores \
  --where data.environment equals production
```

Every aggregate row includes `object_count`. Measures support `sum`, `average`
(`avg` is accepted as an input alias), `min`, and `max` over numeric `data.path`,
`S:key`, or `P:key` values. Filters run before aggregation and accept the same
object fields and dotted data paths as `object list`, plus up to two computed
selectors.
Text output exposes flattened dimension and measure columns; JSON preserves the
server's dimension and measure states, contributing counts, and skipped counts.
Cursor pagination and generated next-page commands operate on aggregate rows.

The `G` and `A` pipe stages are still useful for ad hoc local transformations,
but they only process rows already returned by the preceding command. Use
`object aggregate` when the result must cover the complete server-side matching
set.

Class-specific display aliases provide short local names for raw object-data
paths. Selectors are tried in order and the first present value is displayed:

```toml
[output.object_list_class_aliases.Hosts]
os_version = ["data.os.macos.version", "data.os.redhat.version"]
primary_ipv4 = ["data.network.interfaces[*].ipv4"]
```

The aliases can be included in `output.object_list_class_columns.Hosts` or
requested with `--data-columns`. An unambiguous alias is also used as the text
table header when its raw selector is included automatically, such as by an
object-list `--where` clause. Configure aliases from the CLI with the alias as
the final key component and its selectors as a comma-separated value:

```sh
hubuum-cli config set \
  --key output.object_list_class_aliases.Hosts.IPv4 \
  --value data.facts.network.default_ipv4.address
```

The former
`output.object_list_class_meta` name remains accepted for existing config files
and config commands, but new writes use `object_list_class_aliases`.

Administrators can create full-system backups and perform the server's two-step restore
flow. Format 5 excludes password hashes and bearer tokens, but contains privileged
integration configuration. Backup and receipt files are saved atomically with
owner-only permissions on Unix; existing files require `--force` before replacement:

```sh
hubuum-cli backup create --file hubuum-backup.json
hubuum-cli backup submit
hubuum-cli backup show 123
hubuum-cli backup download 123 --file hubuum-backup.json

hubuum-cli restore stage --file hubuum-backup.json --receipt restore-receipt.json
hubuum-cli restore status --receipt restore-receipt.json
hubuum-cli restore confirm --receipt restore-receipt.json --yes --wait
hubuum-cli restore wait --receipt restore-receipt.json --timeout 600
```

Confirmation queues replacement of all Hubuum data. Use `--wait` or `restore wait`
to verify completion; status and wait use the receipt without logging in, even
after existing bearer tokens are invalidated. After success, reset a local
administrator password with `hubuum-admin --reset-password admin` and issue fresh
tokens. Keep the receipt until recovery is complete. See the
[backup and restore guide](backup-restore.md) for server setup, backup-version
compatibility, larger backups, and recovery instructions.
