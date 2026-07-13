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

- Commands should return semantic values before formatting.
- Pipes should run before final rendering.
- Selectors should mean the same thing across filter, projection, sort, and value extraction.
- Bare tokens can be permissive search; dotted/indexed paths should be strict selectors.
- Real `null` values are data and must survive transforms.
- Transforming verbs should be explicit about shape changes.
- Table formatting should be a renderer concern, not a pipe parser concern.

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
- `V pattern` value-only search and `K pattern` key-only search
- `P field other.nested[]` projections
- `S field`, `S !field`, and typed sorting for numbers, strings, and IPs
- `VALUE field` for plain value extraction
- `G field`, `A count`, grouped `C`, `Z`, and `U field` collection stages
- `JQ` jq-compatible transforms evaluated by the in-process `jaq` interpreter

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

Redirect operators are standalone, whitespace-delimited tokens. Compact DSL
comparisons such as `F age>3` are not redirect candidates. Redirect parsing is
also validated against the preceding command, so command filters such as
`--where age > 3` remain part of the command when truncating at `>` would make
the command invalid. POSIX one-shot invocations must escape or quote `|`, `>`,
and `>>` so the shell passes them to the CLI.

## Remaining Boundaries

- Some command-specific branches still append direct rendered lines. Those
  branches continue to support conservative line stages: `grep`, `reject`,
  `head`, `tail`, `sort line`, and `count`.
- New command output should use shared semantic emitters rather than
  `append_line` for normal result data.
- Do not add behavior that depends on parsing table glyph output.
