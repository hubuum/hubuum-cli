# Output Pipeline Direction

This CLI should move toward a semantic output pipeline:

```text
command result
  -> intermediate JSON value
  -> optional pipe/DSL transforms
  -> final renderer: table, text, json, markdown, values
```

The current `crates/hubuum-filter` implementation is transitional. It gives the
REPL and one-shot commands a stable pipe grammar now, but most stages still
operate on rendered lines. New behavior should avoid making rendered text the
long-term extension point.

## User Model

Users should be able to ask small local questions without adding one more API
flag for every report shape:

```text
object list --class Hosts | F contact | L 5
object list --class Hosts | P name json_data.contact | S !name
object show --class Hosts host-1 | VALUE json_data.contact.email
```

Short aliases should stay consistent with their long names:

- `F` / `grep`: keep matching rows or branches
- `P` / `columns`: project fields
- `S` / `sort`: sort rows
- `L` / `head`: limit rows
- `C` / `count`: count rows

The legacy shorthand remains valid:

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

## Implementation Sketch

Introduce a semantic output value, likely based on `serde_json::Value`, with
small wrappers for row sets, detail documents, warnings, errors, and pagination
metadata. Existing formatters can first populate this value, then renderers can
turn it into text, tables, JSON, or value lists.

`hubuum-filter` should stay CLI-agnostic enough that it can become an
independent crate later. Keep command catalog, authentication, Hubuum client
types, terminal rendering, and REPL concerns outside that crate.

Once commands produce semantic output snapshots, pipe stages can evolve from
line transforms into data transforms:

- `F field=value`, regex, existence, and comparison filters
- `P field other.nested[]` projections
- `S field`, `S !field`, and typed sorting for numbers, strings, and IPs
- `VALUE field` for plain value extraction
- `JQ` or jq-like transforms as a later optional stage

This also gives table rendering more control: projection changes visible
columns before rendering, sorting works on values rather than glyphs, and JSON
output can show the transformed payload without re-parsing terminal text.

## Transitional Boundaries

Until the semantic layer exists:

- Keep rendered-line stages useful and conservative: `grep`, `reject`, `head`,
  `tail`, `sort line`, and `count`.
- Parse structured stages such as `P` and column `S` so user syntax can settle.
- Return a clear error when a structured stage is applied to rendered text.
- Do not add complex behavior that depends on parsing table glyph output.
