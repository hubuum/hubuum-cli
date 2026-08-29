# Hubuum Pipe DSL

Hubuum CLI commands can be followed by pipe stages that filter, reshape, group,
aggregate, and extract semantic output before final rendering.

```text
command output
  -> pipe stages
  -> renderer: table, text, json, jsonl, csv, tsv
  -> optional redirect
```

The DSL is useful when a command already returns the right kind of data and you
want a smaller local view without adding another API flag.

The next native-language increment is specified in
[RFC 0001](rfcs/0001-native-pipeline-dsl.md) and is landing in independently
reviewable slices. Typed boolean predicates are implemented; later RFC sections
remain proposals until their linked delivery issues land.

Grouping with `G` and aggregation with `A` are local pipe operations over the
rows returned by the preceding command. For permission-scoped aggregation over
the complete matching object set before pagination, use `object aggregate`; see
the main README for command examples.

Examples below use REPL/script syntax. In a POSIX shell, escape or quote `|`,
`>`, and `>>` so those standalone operator arguments reach Hubuum CLI, for
example `hubuum-cli config show \| F output \> output.txt`.

## Quick Recipes

Keep Host objects whose OS version contains `26`:

```text
object list --class Hosts | F os_version 26
```

Show a few Host fields:

```text
object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4
object list --class Hosts --computed S:average_load --computed P:note | P Name S:average_load P:note
```

Sort by a numeric data field:

```text
object list --class Hosts | S data.cpu.cores AS num
object list --class Hosts --computed S:average_load | S S:average_load desc AS num
```

Group Hosts by OS version and count them:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts
```

Sort aggregate output by the aggregate number:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num
```

Extract only IPv4 values:

```text
object list --class Hosts | VALUE data.network.interfaces[*].ipv4
```

Write one JSON file per Host:

```text
object list --json --class Hosts | P Name os_version > each:hosts/{Name}.json
```

## Search And Filter

Bare text and the one-argument `F` form are broad quick searches over key paths
and all semantic values, including values not selected as visible table
columns. Matches found only in hidden values are reported as `value` in the
Match column when visible-column metadata causes that column to be generated.

```text
object list --class Hosts | 129.240
```

The two-argument `F <field> <regex>` form searches one field. Compact embedded
operators provide equality, regex, and numeric/string comparisons without
being confused with a standalone redirect operator:

```text
object list --class Hosts | F 129.240
object list --class Hosts | F os_version 26
object list --class Hosts | F data.cpu.cores>=8
object list --class Hosts | F data.network.interfaces[*].ipv4 '^129\.240\.'
object list --class Hosts --computed S:average_load | F S:average_load>=1
object list --class Hosts --computed P:note | F P:note '^mine$'
```

`V` searches values only:

```text
object list --class Hosts | V 129.240
```

`K` searches keys only and returns the matched key projection:

```text
object list --class Hosts | K ipv4
```

`reject` removes matching rows:

```text
object list --class Hosts | reject os_version '^9'
```

`F WHERE` starts the typed predicate grammar. It preserves quoted strings and
JSON literal types, supports `NOT`, `AND`, `OR`, and parentheses, and leaves all
legacy filter forms unchanged. Precedence is parentheses, `NOT`, a field test,
`AND`, then `OR`:

```text
object list --class Hosts | F WHERE data.cpu.cores AS num >= 8 AND state IN ["ready", "running"]
object list --class Hosts | F WHERE NOT (owner IS MISSING OR owner IS NULL)
object list --class Hosts | reject WHERE status == "retired" OR disabled == true
```

Typed tests support `=`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `~`, `!~`,
`MATCHES`, `[NOT] IN [...]`, `IS [NOT] NULL`, and `IS [NOT] MISSING`.
Keywords are ASCII case-insensitive. Double-quoted strings use JSON escapes;
single-quoted strings accept `\\` and `\'` and otherwise preserve their text.
Numbers, booleans, and `null` are unquoted JSON literals; strings must be
quoted, so `3` differs from `"3"`.

An optional `AS str|num|bool|ip|datetime|version|natural` cast makes conversion
explicit. Invalid literals fail while parsing. An invalid selected value stops
evaluation and reports the stage, selector, cast, 1-based row, and offending
JSON value. Boolean evaluation short-circuits left to right.

Positive comparisons, matches, and `IN` are existential for fanout selectors.
Their negative forms require at least one selected value and require every
selected value to pass, so a missing selector does not satisfy `!=`, `!~`, or
`NOT IN`. `IS NULL` detects selected JSON null; `IS MISSING` detects no selector
matches. Their `IS NOT` forms and general `NOT` are exact logical negations.

`?` removes empty values, or keeps rows where a selector is truthy:

```text
object list --class Hosts | ?
object list --class Hosts | ? data.network.interfaces[]
```

## Projection And Values

`P` selects columns. Selectors can be separated by spaces or commas.

```text
object list --class Hosts | P Name os_version
object list --class Hosts | P Name,data.cpu.cores
object list --class Hosts | P Name data !data.secrets
object list --class Hosts | P data.network.interfaces !data.network.interfaces[].mac
object show --class Hosts host-1 --computed S:average_load --computed P:note | P Name S:average_load P:note
```

Prefix a selector with `!` to remove its matches from the projected value. Drop
terms use the same dotted fields, indexes, negative indexes, fanout, and slices
as keep terms. A terminal index removes that array element, a terminal slice
removes the selected range, and a terminal `[]` or `[*]` empties the selected
array. Missing paths are harmless. Array traversal must be explicit, so use
`!items[].secret`, not `!items.secret`, to remove a field from every item.
Repeating the same projected output column is an error.

Shared and personal computed fields use the ordinary top-level selectors
`S:<key>` and `P:<key>` after they are selected with the repeatable
`--computed` option (or `--computed all`). Their underlying JSON types are
retained for semantic pipe operations. Computation errors are represented as
`ERROR: ...` strings.

Selections configured in `output.object_class_computed_fields.<class>` are
available to the pipeline automatically. Explicit `--computed` values replace
the class default; `--computed none` disables it for one command.

`VALUE` and `VAL` extract selected leaves as a value list:

```text
object list --class Hosts | VALUE data.network.interfaces[*].ipv4
object list --class Hosts | VAL Name
```

## Sorting, Limits, And Counts

Sort ascending by default:

```text
object list --class Hosts | S os_version
```

Use `!` or `desc` for descending order:

```text
object list --class Hosts | S !os_version
object list --class Hosts | sort os_version desc
```

Use casts when text ordering is not right:

```text
object list --class Hosts | S data.cpu.cores AS num
object list --class Hosts | S data.network.interfaces[0].ipv4 AS ip
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num
```

Limit rows:

```text
object list --class Hosts | L 10
object list --class Hosts | L 10 20
object list --class Hosts | tail 5
```

Count rows:

```text
object list --class Hosts | F os_version 26 | C
```

## Grouping And Aggregates

These stages operate locally on the current semantic rows. They do not call the
server's object-aggregate endpoint.

`G` establishes a grouped-state boundary. Stages before `G` operate on member
rows. Stages after `G` operate on each visible summary, which is the flattened
set of group aliases and aggregate aliases:

- `F`, `V`, `reject`, and `? field` keep or remove whole groups by testing the
  visible summary. They never edit hidden members. A member-only selector after
  `G` therefore matches no group.
- `K` projects matching summary key paths, while `P` projects selected summary
  fields. `U` unrolls a summary array into multiple groups with the same member
  rows. `S`, `L`, and `tail` order or limit whole groups.
- `A` reads the unchanged members of each retained group, so aggregates added
  before or after a summary filter remain accurate. `Z` emits one visible row
  per retained group.
- Grouped `C` is a terminal collapse that emits one summary row per group with
  a `count` field for its member count.

Grouping never creates an empty group, and summary filters remove whole groups,
so they cannot make a retained group member-empty. A pre-existing empty group
is retained or removed solely by the same visible-summary predicate as any
other group.

Group by one or more fields:

```text
object list --class Hosts | G os_version AS "OS Version"
object list --class Hosts | G os_version AS "OS Version" data.cpu.arch AS Architecture
```

Group aliases must be unique. Aggregate aliases cannot reuse a group alias or
an earlier aggregate alias; collisions fail instead of overwriting a visible
value while leaving duplicate column metadata.

Array selectors fan out group membership:

```text
object list --class Hosts | G data.network.interfaces[*].ipv4 AS IPv4
```

Aggregate grouped rows:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts
object list --class Hosts | G os_version AS "OS Version" | A sum(data.cpu.cores) AS Cores
object list --class Hosts | G os_version AS "OS Version" | A avg(data.cpu.cores) AS "Average Cores"
object list --class Hosts | G os_version AS "OS Version" | A min(Name) AS First
object list --class Hosts | G os_version AS "OS Version" | A max(Name) AS Last
```

Aggregates are ordinary visible output columns, so later stages can filter,
sort, project, or redirect them:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num | L 10
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | F Hosts>=2
```

`C` after grouping returns one count row per group:

```text
object list --class Hosts | G os_version AS "OS Version" | C
```

`Z` collapses grouped output to group and aggregate columns:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | Z
```

Before `G`, `U` unrolls member arrays into rows:

```text
object list --class Hosts | U data.network.interfaces | P Name ipv4 mac
```

After `G`, `U` instead unrolls an array in the visible group summary and keeps
the original member rows attached to every resulting group.

## Shape Contracts

Every pipeline value has one of seven shapes: `Empty`, `Lines`, `Rows`,
`Detail`, `Message`, `Values`, or `Groups`. A stage validates its input shape
before doing any work. Unsupported combinations fail with the stage name, the
current shape, and every accepted shape; a transforming stage never silently
passes through non-empty input.

The table is the complete input and result contract. `same` retains the input
shape, `/E` means the predicate may produce `Empty`, and `dynamic` means JQ
derives `Empty`, `Rows`, `Detail`, `Message`, or `Values` from its JSON result.

| Stage | Empty | Lines | Rows | Detail | Message | Values | Groups |
| --- | --- | --- | --- | --- | --- | --- | --- |
| bare, legacy `F`, legacy `reject` | same | same | same | same/E | same/E | same | same |
| `F WHERE`, `reject WHERE` | same | error | same | same/E | same/E | same | same |
| `V` | same | same | same | same/E | same/E | same | same |
| `K` | same | error | Rows | Detail/E | Detail/E | Rows | Groups |
| `?` | same | error | same | same/E | same/E | same | same |
| `L`, `head`, `tail` | same | same | same | error | error | same | same |
| `C`, `count` | Values | Values | Values | Values | Values | Values | Rows |
| whole-line `S`, `sort` | same | same | same | error | error | same | same |
| field `S`, `sort` | same | error | same | error | error | same | same |
| `P`, `columns` | same | error | Rows | Detail | Detail | error | Groups |
| `G` | Groups | error | Groups | Groups | Groups | Groups | error |
| `A` | error | error | error | error | error | error | Groups |
| `Z` | error | error | error | error | error | error | Rows |
| `U` | same | error | same | error | error | same | same |
| `JQ` | dynamic | error | dynamic | dynamic | dynamic | dynamic | dynamic |
| `VALUE`, `VAL` | Values | error | Values | Values | Values | Values | Values |

`Empty` is an intentional identity only for stages that retain, order, limit,
project, or unroll an existing collection. `C`, `G`, `JQ`, and `VALUE` make an
explicit shape transition from `Empty`; `A` and `Z` still require `Groups`.
`P` and `K` turn a structured `Message` into `Detail` because projection removes
message presentation semantics. Grouped `C` is the special summary-row result
described above.

## Line-shaped Output

Most command results enter the pipeline as semantic rows, details, messages, or
values regardless of the selected renderer. Commands whose result is inherently
prose or a text stream enter as an explicit `Lines` shape. Lines support broad
or value regex filtering (legacy `F`, `V`, bare filters, and legacy `reject`),
`head`/`L`, `tail`, `C`, and whole-line `S`/`sort`. Typed predicates and other
field-aware stages fail because lines do not contain structured fields.

`C` turns `Lines` into a one-element `Values` result containing the numeric line
count. Other supported line stages retain the `Lines` shape.

After a line pipeline, text emits the retained lines, JSON emits an array of
strings, JSONL emits one JSON string per line, and CSV/TSV emit a `value` column.
Selecting one of those renderers does not change which lines reach the stages.

## JQ

`JQ` evaluates a jq-compatible expression against the current semantic payload
using the in-process `jaq` interpreter:

```text
object list --json --class Hosts | JQ 'map({Name, os_version})'
object list --json --class Hosts | JQ '.[] | .Name'
```

Zero jq outputs become empty semantic output, one output keeps its natural
shape, and multiple outputs are collected into semantic rows or values. JQ
clears the existing visible-column metadata and infers the result shape.

Prefer built-in stages for common filtering, grouping, and projection because
they preserve Hubuum table metadata and completions.

## Selectors

Selectors are shared by filter, projection, sorting, grouping, and value
extraction.

```text
Name                              top-level field
os_version                        top-level field
data.owner                        dotted path
data.network.interfaces[0]        array index
data.network.interfaces[-1]       negative index
data.network.interfaces[*]        array fanout
data.network.interfaces[]         array fanout
data.network.interfaces[:2]       slice
```

Dotted and indexed selectors are strict path lookups. Bare quick search remains
permissive and can match keys or values. Malformed selectors fail while the
pipeline is parsed, before any stage changes the data. This includes empty path
components, unmatched brackets, invalid indexes or slice bounds, and characters
after a closing bracket.

Dots and square brackets are selector syntax. Field names containing those
characters cannot currently be addressed because the DSL does not define an
escape syntax for selector metacharacters. Colons remain ordinary field-name
characters, including in computed-field selectors such as `S:average_load`.

## Redirects

Redirects run after pipe stages:

```text
object list --class Hosts | P Name os_version > hosts.txt
object list --class Hosts | P Name os_version >> hosts.txt
object list --json --class Hosts | P Name os_version > each:hosts/{Name}.json
object list --class Hosts | VALUE Name > each:names/{value}.txt
```

`each:<template>` writes one file per semantic row or value. Placeholders use
the same field names as the current output, plus `{value}` for `VALUE` output
and `{n}` for a 1-based item number. It requires structured semantic output;
parent directories must already exist. Duplicate generated paths are rejected
before any files are written, and placeholder values are sanitized for paths.

Redirect paths support quoting, `~/...` expansion, and REPL file path
completion. The `>` and `>>` operators must be standalone,
whitespace-delimited tokens. Compact comparisons such as `F age>3` remain
filter expressions. A spaced comparison in `F WHERE age > 3` remains part of
the predicate because the prefix before `>` is not a complete pipeline. A
later standalone operator redirects once its preceding pipeline is complete.
Command filters such as `--where age > 3` are likewise retained when the
command before `>` would otherwise be invalid.

Redirect files honor the configured color mode. `auto` and `never` strip ANSI
styling from non-terminal files; `always` preserves it. In one-shot POSIX shell
commands, escape application-level operators, for example:

```sh
hubuum-cli object list --class Hosts \| P Name os_version \> hosts.txt
hubuum-cli object list --class Hosts \| VALUE Name \> each:/tmp/host-{value}.txt
```

## Help

Use focused help topics in the REPL:

```text
help pipe
help pipe search
help pipe project
help pipe sort
help pipe limit
help pipe group
help pipe selectors
help pipe shapes
help pipe redirects
help pipe jq
```
