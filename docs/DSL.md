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
want a smaller local report without adding another API flag.

## Quick Recipes

Keep Host objects whose visible or hidden semantic data mentions an OS version:

```text
object list --class Hosts | F os_version contains 26
```

Show a few Host fields:

```text
object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4
```

Sort by a numeric data field:

```text
object list --class Hosts | S data.cpu.cores AS num
```

Group Hosts by OS version and count them:

```text
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts
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

Bare text is a broad quick search over keys and values:

```text
object list --class Hosts | 129.240
```

`F` filters by a pattern or field predicate:

```text
object list --class Hosts | F 129.240
object list --class Hosts | F os_version contains 26
object list --class Hosts | F data.cpu.cores>=8
object list --class Hosts | F data.network.interfaces[*].ipv4 matches '^129\.240\.'
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
object list --class Hosts | reject os_version contains '^9'
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
```

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
```

Limit rows:

```text
object list --class Hosts | L 10
object list --class Hosts | L 10 20
object list --class Hosts | tail 5
```

Count rows:

```text
object list --class Hosts | F os_version contains 26 | C
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

`JQ` applies a jq-like expression to the current semantic payload:

```text
object list --json --class Hosts | JQ 'map({Name, os_version})'
```

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
```

`each:<template>` writes one file per semantic row or value. Placeholders use
the same field names as the current output, plus `{value}` for `VALUE` output
and `{n}` for a 1-based item number.

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

