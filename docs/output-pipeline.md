# Output Pipeline Direction

This CLI uses a semantic output pipeline for shared formatter output:

```text
command result
  -> intermediate JSON value
  -> optional pipe/DSL transforms
  -> final renderer: table, text, json, jsonl, csv, tsv
  -> optional redirect sink: > file, >> file, > each:<template>
```

`crates/hubuum-filter` owns pipe parsing and semantic transforms. The CLI owns
terminal rendering, config, command dispatch, and REPL behavior.

See [DSL.md](DSL.md) for the delivered user-facing DSL spec and examples.
See [RFC 0001](rfcs/0001-native-pipeline-dsl.md) for the proposed compatible
next language increment and staged delivery plan.

## User Model

Users should be able to ask small local questions without adding one more API
flag for every output shape:

```text
object list --class Hosts | F contact | L 5
object list --class Hosts | P Name data.owner | S !Name
object show --class Hosts host-1 | VALUE data.owner.email
```

Short aliases should stay consistent with their long names:

- `F` / `grep`: keep matching rows or branches
- `P` / `columns`: project fields
- `S` / `sort`: sort rows
- `L` / `head`: limit rows
- `C` / `count`: count rows

The bare-search shorthand remains valid:

```text
object list --class Hosts | contact
```

## Design Rules

- Default text output should summarize large JSON documents with useful counts,
  status, and bounded previews. Keep full payloads available through explicit
  structured output and semantic pipelines.
- Commands should return semantic values before formatting.
- Pipes should run before final rendering.
- Selectors should mean the same thing across filter, projection, sort, and value extraction.
- Bare tokens can be permissive search; dotted/indexed paths should be strict selectors.
- Real `null` values are data and must survive transforms.
- Transforming verbs should be explicit about shape changes.
- Table formatting should be a renderer concern, not a pipe parser concern.

Each stage exposes its accepted input shapes and possible resulting shapes from
`hubuum-filter`. Evaluation checks that contract before applying the stage, and
unsupported combinations report the stage, current shape, and accepted shapes.
The complete matrix is in [DSL.md](DSL.md#shape-contracts).

## Implementation Notes

The semantic output value is based on `serde_json::Value`, with wrappers for
row sets, detail documents, messages, lines, values, visible columns, warnings,
errors, and pagination metadata. Shared formatters populate this value first;
renderers turn it into text, tables, JSON, JSONL, CSV, or TSV after pipeline
transforms have run.

`hubuum-filter` should stay CLI-agnostic enough that it can become an
independent crate later. Keep command catalog, authentication, Hubuum client
types, terminal rendering, and REPL concerns outside that crate.

Pipe stages now run on semantic data when commands use shared formatters:

- `F pattern`, `F field regex`, and compact equality/comparison filters such as
  `F field=value` or `F field>=8`
- `F WHERE predicate` and `reject WHERE predicate` with typed literals,
  boolean composition, null/missing tests, fanout, and explicit casts
- `V pattern` value-only search and `K pattern` key-only search
- `P field other.nested[]` projections
- stable comma-separated `S` sort keys with direction, strict casts, fanout
  reduction, and explicit null placement
- `VALUE field` for plain value extraction
- `G field`, `A count`, grouped `C`, `Z`, and `U field` collection stages
- `JQ` jq-compatible transforms evaluated by the in-process `jaq` interpreter

`G` is a semantic boundary: later search, truthiness, projection, unroll, and
sort stages operate on visible group summaries and retain or transform whole
groups. Only `A` and grouped `C` read the attached member rows, and neither
mutates them.

This also gives table rendering more control: projection changes visible
columns before rendering, sorting works on values rather than glyphs, and JSON
output can show the transformed payload without re-parsing terminal text.

Redirects are handled by the CLI after pipe stages. `>` writes the rendered
snapshot to a file, and `>>` appends it. `each:<template>` is a semantic redirect
sink that writes one file per transformed row or value, with filename
placeholders such as `{Name}`, `{data.owner}`, `{value}`, and `{n}` resolved
before each item is rendered. Parent directories must already exist, and
duplicate generated paths fail before writing. Redirect output uses the same
color choice as terminal output: `auto` and `never` strip ANSI from files,
while `always` retains it.

With `--debug --output json`, final rendering wraps the transformed result in
`{"data": ..., "debug": ...}`. Debug metadata does not enter pipe transforms or
filename-template resolution. File redirects receive that response object, and
each `each:` file contains its own result with the whole-command debug report.
Warnings, errors, and plain-text messages are optional sibling arrays. See
[command diagnostics](commands.md#command-diagnostics) for the report schema and
timing boundary. JSON output without debug keeps its existing shape.

Without debug JSON, warnings and errors go to stderr for JSON, JSONL, CSV, and
TSV output, including file redirects. Sampling warnings therefore remain visible
without corrupting data consumed by another program. Text output keeps notices
alongside the result.

Redirect operators are standalone, whitespace-delimited tokens. Compact DSL
comparisons such as `F age>3` are not redirect candidates. Spaced comparisons
such as `F WHERE age > 3` remain predicates because redirect parsing requires
the preceding pipeline to be complete; a later standalone operator can then
redirect the result. The same validation keeps command filters such as
`--where age > 3` intact. POSIX one-shot invocations must escape or quote `|`,
`>`, and `>>` so the shell passes them to the CLI.

## Remaining Boundaries

Commands with custom text presentations expose their complete response to
pipelines and structured renderers. This includes version information, config
paths and preference updates, theme previews, alias updates, issued tokens,
task output, computed previews, class and group details, and relation graphs.
Normal text invocations retain their readable summaries. Theme previews return
the theme definition, including roles and license, in structured output.

Use the response's field names instead of parsing labels or table glyphs:

```text
version | VALUE cli_version
config paths | VALUE user
theme preview catppuccin-mocha | P name license
class show Hosts | P class.name objects related_classes
group show --groupname operators | P group members
relation object graph --root-class Hosts --root-object host-1 | P objects relations
task output 123 | JQ '.ImportResults'
```

**Pipeline migration:** composite class/group details, relation graphs, and
shared computed-field lists and mutations now provide one complete response
document. For example, use `class.name` for the class name, `group.groupname` for
the group name, and `definitions`/`state` for shared computed lists. Select
`definition`/`state` for a computed mutation. Mutation confirmations expose
`message`; token responses expose `token` and `expires_at`. Existing list-table
selectors such as `Name` remain available on ordinary list commands.

Human-only output, such as help text, retains line stages: `grep`, `reject`,
`head`, `tail`, `sort line`, and `count`. New result data should use shared
semantic emitters. Reserve `append_line` for text presentation and auxiliary
messages; auxiliary lines accompanying semantic data are excluded from JSON,
JSONL, CSV, and TSV payloads. JSON debug responses retain them in `messages`.

## Table controls

Table rendering can be tuned per run or with config keys:

```sh
hubuum-cli --table-style plain object list --limit 5
hubuum-cli --table-style dense --table-bands auto object list --limit 5
hubuum-cli --table-width full --table-wrap 40 object list --class Hosts
hubuum-cli --empty-result silent object list --class Hosts --where name equals __no_matching_host__
hubuum-cli object list --class Hosts --table-headers full
hubuum-cli object list --class Hosts --table-headers none
```

The empty-result example assumes no object is named `__no_matching_host__`.
Page sizes must be positive; `--limit 0` is rejected before lookup.

Grouped headers are the default for text tables. Dotted paths stay on one line
when they fit the column, or wrap between path segments when space is limited.
Compact columns keep their widths based on row content and individual path
segments; unambiguous class aliases take precedence. Use `--table-headers full`
for the original flat paths, or `--table-headers none` to suppress table headers.
Machine-oriented formats retain their semantic column names. Persist the mode
with `hubuum-cli config set --key output.table_headers --value none`.

Related config keys are `output.table_style`, `output.table_headers`,
`output.table_width`, `output.table_wrap`, `output.table_bands`, and
`output.empty_result`.

Large payload options can read from explicit value sources. This is opt-in per option, so ordinary values such as remote target URLs remain literal.

```sh
hubuum-cli object create --name item-1 --class Device --collection main --description "imported" --data file://payload.json
hubuum-cli class create --name Device --collection main --description "devices"
hubuum-cli class schema stage --class Device --schema https://example.com/schema.json --validate true
```
