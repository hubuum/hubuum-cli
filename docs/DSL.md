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
object show --class Hosts host-1 --computed S:average_load --computed P:note | P Name S:average_load P:note
```

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

Group by one or more fields:

```text
object list --class Hosts | G os_version AS "OS Version"
object list --class Hosts | G os_version AS "OS Version" data.cpu.arch AS Architecture
```

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

Aggregates are ordinary output columns, so later stages can sort, project, or
redirect them:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num | L 10
```

`C` after grouping returns one count row per group:

```text
object list --class Hosts | G os_version AS "OS Version" | C
```

`Z` collapses grouped output to group and aggregate columns:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | Z
```

`U` unrolls array members into rows:

```text
object list --class Hosts | U data.network.interfaces | P Name ipv4 mac
```

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
permissive and can match keys or values.

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
filter expressions, and command filters such as `--where age > 3` are retained
when the command before `>` would otherwise be invalid.

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
help pipe redirects
help pipe jq
```
