# Extension JSONC reference

Every package contains `hubuum-extension.jsonc`. The CLI accepts comments and
trailing commas, while retaining strict JSON strings, keys, numbers, and comma
rules. Use comments to document why a site-specific value or workflow step
exists; the compiler ignores them, so they have no runtime semantics. Unknown
fields are errors. Add the repository's
[`hubuum-extension.schema.json`](../schemas/hubuum-extension.schema.json) as
`$schema` for editor validation and completion.

The schema catches structural mistakes while editing. `extension validate`
remains authoritative because cross-references, command contracts, effects,
call graphs, configuration, and work limits require the compiler.

## Top-level fields

| Field | Required | Meaning |
| --- | --- | --- |
| `$schema` | No | Editor-only URI or relative path to the JSON Schema |
| `schema_version` | Yes | Manifest language version; currently `1` |
| `kind` | Yes | `portable` or `executable` |
| `name` | Yes | Pack namespace in lowercase kebab-case |
| `version` | Yes | Pack semantic version |
| `requires_cli` | Yes | SemVer requirement checked before registration |
| `config` | No | Typed, pack-local configuration declarations |
| `workflows` | Portable | Reusable same-pack workflow declarations |
| `protocol` | Executable | Exactly `hubuum-cli.extension/v1` |
| `executable` | Executable | Relative program path confined to the package |
| `commands` | Yes | User-visible command declarations |

Portable packs cannot declare `protocol` or `executable`. Executable packs
cannot declare workflows. Pack names reserve all built-in `extension`
management names.

## Names and paths

- Pack names, command path segments, and long options use lowercase ASCII
  kebab-case: `site-inventory`.
- Command declaration keys, workflow names, configuration keys, workflow
  inputs, and step IDs use lowercase ASCII snake_case: `list_hosts`.
- Option declaration and binding IDs allow lowercase ASCII letters, numbers,
  hyphens, and underscores. A portable option matches a snake-case workflow
  input key exactly.
- Command paths are non-empty arrays such as `["host", "list"]`.
- One command path cannot prefix another command path in the same pack.
- Short and long aliases share a namespace. Host rendering aliases are
  reserved.

## Value declarations

Configuration and workflow inputs use the same fields:

| Field | Default | Meaning |
| --- | --- | --- |
| `type` | Required | `string`, `integer`, `number`, `boolean`, or `json` |
| `required` | `false` | Caller or local configuration must supply a value |
| `repeatable` | `false` | The resolved value is an array of the declared type |
| `default` | None | Typed value used when omitted |
| `help` | Empty | User-facing explanation |

`required` and `default` are mutually exclusive. A repeatable default is an
array. Unknown local configuration keys and wrong types reject the pack.

## Workflows

A workflow object has:

| Field | Required | Meaning |
| --- | --- | --- |
| `inputs` | No | Typed public function inputs |
| `output` | Yes | Static semantic output declaration |
| `steps` | Yes | Ordered tagged step array |
| `capabilities` | No | Include `mutate` when the expanded graph may write |
| `result` | Yes | Bounded JQ expression producing the final value |

Workflows are top-level so multiple commands and workflows can reuse them.
Only same-pack calls are legal. The compiler expands the complete call graph,
detects cycles, propagates effects and replay safety, and checks all static
limits before registering any command.

### Output shapes

| `shape` | Required JQ result | Typical use |
| --- | --- | --- |
| `empty` | `null` or `[]` | No semantic value |
| `lines` | Array of strings | Unstructured display lines |
| `rows` | Array of JSON objects | Lists and table output |
| `detail` | JSON object | One structured record or report |
| `message` | JSON scalar | Status text, number, Boolean, or `null` |
| `values` | JSON array | Non-row arrays |

`type` defaults to `json`. It validates individual elements for `lines` and
`values`, and the complete result for the other shapes; `rows` and `detail`
therefore normally use `json`. `columns` is an optional unique list of non-empty
display columns for `rows` or `detail`. The result is checked before the
caller's pipeline, renderer, and redirect run.

### Tagged steps

Every step requires a unique snake-case `id`. Array order is execution order.

| Kind | Required fields | Optional fields | Result |
| --- | --- | --- | --- |
| `run` | `run` command path | `with`, `when` | Built-in semantic value |
| `let` | `expr` | None | JQ result |
| `assert` | `condition`, `message` | None | `true`, or stops with message |
| `call` | `call` workflow | `with`, `when` | Nested workflow result |
| `for_each` | `items`, `as`, `call`, `max_items` | `with`, `when` | Result array |

`when` is available only on executable steps: `run`, `call`, and `for_each`.
It must return a Boolean. A false condition records a skipped `null` value.
`for_each` is sequential and requires `max_items` from 1 through 1,000. The
`as` name binds each item to an input on its target workflow.

Use `extension contract <built-in path>` to discover the accepted `with` keys,
types, required state, flags, fixed or repeatable cardinality, effects,
authentication, and replay safety for a `run` target.

### Bindings

A `with` object maps target input IDs to one of these forms:

| Form | Example | Meaning |
| --- | --- | --- |
| Scalar literal | `"Hosts"` | String, number, Boolean, array, or `null` |
| Object literal | `{ "literal": { "site": "oslo" } }` | Explicit JSON object |
| Input | `{ "input": "class" }` | Current workflow input |
| Configuration | `{ "config": "hosts_class" }` | Declared pack configuration |
| Earlier step | `{ "step": "classes" }` | Normalized earlier step value |
| Selected step | `{ "step": "classes", "select": ".[0].name" }` | Bounded JQ projection |

Forward references are errors. A plain object is reserved for binding syntax,
so use the explicit `literal` wrapper for object data. Dynamic step values are
type-checked immediately before invocation.

## Commands and options

A command requires `path`. Portable commands require `workflow`; executable
commands instead inherit the package executable and may declare fixed
`arguments` and `interactive`.

Both kinds accept `about`, `long_about`, `examples`, and an `options` object.
Each `examples` entry contains only the arguments after this command's path;
the help renderer prepends the complete installed path. Use an empty string to
show the bare command.
An option has `kind`, and then one of `position`, `long`, or `short` as its
interface. It may also declare `required`, `repeatable`, `help`, and static
`values`. Supported kinds are `string`, `integer`, `number`, `boolean`, `flag`,
and `json`.

Positional indexes start at one and must be contiguous. Only the last
positional may repeat. A portable command's option declarations must exactly
match its workflow inputs by declaration key, type, required state, and
repeatability. A workflow without a command remains private.

## JQ context and limits

Expressions see:

- `.input`: resolved typed workflow inputs;
- `.config`: resolved typed pack configuration;
- `.steps`: normalized step values keyed by ID;
- `.outputs`: source and semantic-output metadata keyed by step ID.

The bounded expression profile rejects user definitions and variables,
recursive descent, multiplication, multiple array generators, `combinations`,
`walk`, `recurse`, `repeat`, `range`, `while`, `until`, `foreach`, and `reduce`.

The v1 host limits are mandatory:

| Resource | Limit |
| --- | ---: |
| Expanded call depth | 16 |
| Static and runtime operations | 10,000 |
| One `for_each` | 1,000 items, further reduced by `max_items` |
| Cumulative workflow output | 4 MiB |
| JQ expression source | 4 KiB |
| JQ expression input | 1 MiB |
| JQ expression results | 128 |
| JQ expression output | 1 MiB |

Mutating expanded graphs require `"capabilities": ["mutate"]` at every
calling boundary. Workflows do not supply transactions or rollback.

## Executable packs

Executable packs exist for external I/O or behavior that built-in commands and
bounded JQ cannot express. They may use any implementation but must treat that
implementation and its dependencies as part of installation.

The CLI executes the declared program directly, without a shell, followed by
fixed command `arguments` and validated caller arguments. Only a declared
interactive command can inherit a terminal stdin. The child receives the
protocol, pack name, current CLI path, non-secret connection settings, and only
its namespaced configuration. Password and bearer token values are removed.

The child writes diagnostics to stderr and exactly one tagged JSON response to
stdout. A success has this form:

```json
{
  "protocol": "hubuum-cli.extension/v1",
  "status": "ok",
  "output": {
    "shape": "message",
    "value": "done",
    "columns": []
  },
  "warnings": []
}
```

An error uses nonzero exit status plus `status: "error"`, with a stable
snake-case `error.code`, user-facing `message`, and optional JSON `details`.
A mismatched exit status, malformed JSON, unsupported protocol, or invalid
shape is a protocol error.
