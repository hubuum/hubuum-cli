# Extension command packs

Extension packs add trusted, site-specific workflows to the Hubuum CLI command
catalog without compiling them into the CLI. A manifest-only workflow invokes
built-in commands in-process and needs no runtime other than
`hubuum-cli`. Packs may also declare executable-backed commands implemented in
Bash, Python, Rust, or another language. Both forms participate in help, REPL
scopes, declared option validation, static completion, semantic pipelines,
output formats, and redirects.

Extension commands use a reserved namespace:

```console
hubuum-cli extension host show server-01
hubuum-cli extension host move server-01 J-42
```

Built-in management names are reserved and packs cannot shadow built-in
commands. Pack discovery is deterministic; duplicate pack names quarantine all
copies rather than selecting one based on load order.

## Discovery and configuration

The default system and user extension roots are `extensions.d` next to the
corresponding Hubuum CLI configuration file. Missing roots are ignored. Both
roots are searched and discovered packs are enabled by default. Override them
with absolute paths when an installation has a different layout:

```toml
[extensions]
system_roots = ["/opt/hubuum/extensions.d"]
user_roots = ["/home/alice/.config/hubuum/extensions.d"]
disabled = ["retired-pack"]

[extensions.config.host]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
host_collection = "inventory"
default_jack = "J-000"
```

Each immediate, non-hidden child directory is one package. Hidden directories
are reserved for staging and trash. Symlinked package directories are ignored
and reported by `extension doctor`. Extension roots, disabled state, and pack
configuration are local configuration; they are not included in portable
personal preference export.

Only the named table under `extensions.config.<pack>` is serialized for that
pack. The complete effective application configuration is never exposed.

## Package manifest

A manifest-only package contains one file:

```text
site-inventory/
└── hubuum-extension.toml
```

An executable-backed package also contains the executable referenced relative
to the manifest:

```text
site-inventory/
├── hubuum-extension.toml
└── bin/
    └── site-inventory
```

The v1 executable-backed manifest is TOML:

```toml
schema_version = 1
kind = "executable"
name = "site-inventory"
version = "1.2.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/site-inventory"

[[commands]]
path = ["host", "show"]
arguments = ["host", "show"]
about = "Show a Host and its physical placement"
examples = ["extension site-inventory host show server-01"]

[[commands.options]]
name = "identifier"
kind = "string"
positional = true
required = true
help = "Host name or installation-specific identifier"

[[commands.options]]
name = "view"
kind = "string"
long = "view"
help = "Output detail level"
values = ["summary", "full"]
```

Pack names, command path segments, and long option names use lowercase ASCII
kebab-case. The executable path must remain within the package. Supported
option kinds are `string`, `integer`, `number`, `boolean`, `flag`, and `json`.
Positionals are ordered; only the final positional may be repeatable. The
`values` list provides validation and static completion.
One command path cannot be a prefix of another command path in the same pack.
Short and long option aliases share one namespace after removing their dashes,
and aliases reserved by host rendering options are rejected. Manifest schema
versions are strict: unknown top-level, command, option, workflow, step, and
binding fields are rejected so misspellings cannot silently change behavior.
Each command has exactly one implementation. `interactive` and executable
`arguments` are invalid on workflow commands.

## Portable workflows

A portable pack is explicitly classified with `kind = "portable"`. It cannot
declare `protocol` or `executable`; its only runtime dependency is the current
`hubuum-cli`. An executable pack uses `kind = "executable"`, must declare both
process fields, and cannot mix in workflow declarations.

Workflows are reusable top-level declarations. Commands expose selected
workflows, while other workflows remain private building blocks in the same
pack:

```toml
schema_version = 1
kind = "portable"
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"

[config.hosts_class]
type = "string"
default = "Hosts"
help = "Class containing Host objects"

[workflows.snapshot]
result = "{ hosts: .steps.hosts, first: .steps.first }"

[[workflows.snapshot.inputs]]
name = "enabled"
type = "boolean"
default = true

[workflows.snapshot.output]
shape = "detail"
type = "json"
columns = ["hosts", "first"]

[[workflows.snapshot.steps]]
id = "hosts"
kind = "run"
run = ["object", "list"]
when = ".input.enabled"

[workflows.snapshot.steps.with]
class = { config = "hosts_class" }
all = true

[[workflows.snapshot.steps]]
id = "first"
kind = "let"
expr = ".steps.hosts[0]"

[[workflows.snapshot.steps]]
id = "has_hosts"
kind = "assert"
condition = "(.steps.hosts | length) > 0"
message = "No Hosts were found"

[[commands]]
path = ["snapshot"]
workflow = "snapshot"
about = "Collect Hosts"

[[commands.options]]
name = "enabled"
kind = "boolean"
long = "enabled"
```

The command option interface must exactly match the referenced workflow's
public inputs by name, type, required state, and repeatability. Internal
workflows need no command. Input and configuration types are `string`,
`integer`, `number`, `boolean`, and `json`. Declarations can be required,
repeatable, or have a typed default. Unknown configured keys, missing required
values, and type mismatches quarantine an installed pack.

### Tagged steps

Every step has a unique snake-case `id` and one explicit `kind`:

| Kind | Purpose | Conditional |
| --- | --- | --- |
| `run` | Invoke a built-in Hubuum CLI command in-process | `when` |
| `let` | Store the result of a JQ expression | Always |
| `assert` | Require a JQ expression to return `true` | Always |
| `call` | Invoke another workflow in this pack | `when` |
| `for_each` | Invoke one same-pack workflow for every array item | `when` |

`run` binds values to canonical input IDs from the built-in command catalog.
Authors do not construct shell commands, spell flags, or order positional
arguments. `call` and `for_each` bind declared workflow inputs. Cross-pack
workflow calls are prohibited. `for_each` requires both an `as` input name and
a positive `max_items` bound:

```toml
[[workflows.snapshot.steps]]
id = "details"
kind = "for_each"
items = { step = "hosts" }
as = "host"
call = "describe_host"
max_items = 100
when = ".input.enabled"

[workflows.snapshot.steps.with]
verbose = true
```

A `with` value may be:

- a literal TOML string, number, boolean, or array;
- `{ literal = { key = "value" } }` for a literal object;
- `{ input = "name" }` for a workflow input;
- `{ config = "key" }` for declared pack configuration;
- `{ step = "id", select = "jq expression" }` for an earlier step value.

Configuration defaults belong to `[config.<key>]`, not individual bindings.
Forward step references are rejected. Dynamically selected values receive
target validation immediately before execution.

### Expressions and outputs

JQ is the workflow expression language. `let`, `assert`, `when`, step
selectors, and the mandatory workflow `result` all evaluate against JSON.
Workflow expressions see:

- `.input` for resolved typed inputs;
- `.config` for resolved typed pack configuration;
- `.steps` for normalized values keyed by step ID;
- `.outputs` for source and semantic-output metadata.

The workflow output declaration is mandatory. It fixes the semantic `shape`,
item or scalar `type`, and optional columns before execution. Supported shapes
are `empty`, `lines`, `rows`, `detail`, `message`, and `values`. The final JQ
value must satisfy that contract; the caller's pipeline and output renderer run
only after validation.

Workflow JQ uses a bounded profile. User definitions and variables, recursive
descent, multiplication, multiple array generators, `combinations`, `walk`,
`recurse`, `repeat`, `range`, `while`, `until`, `foreach`, and `reduce` are
rejected. This profile is intentionally smaller than JQ available in ordinary
CLI pipelines.

### Compilation and limits

The CLI compiles every portable pack into a stable `WorkflowPlan` intermediate
representation before registering commands. Compilation resolves built-in
command contracts, expands the same-pack call graph, detects cycles, computes
effects and retry safety, and rejects graphs that exceed fixed host limits.
Installed failures quarantine the whole pack and appear in `extension doctor`.

The v1 limits are mandatory and cannot be weakened by a pack:

- call depth: 16;
- worst-case and runtime operations: 10,000;
- one `for_each`: 1,000 items, further restricted by its `max_items`;
- cumulative workflow output: 4 MiB;
- one JQ expression: 4 KiB source, 1 MiB input, 128 outputs, and 1 MiB output.

Static checks and runtime counters both enforce the limits. Iteration is
sequential and deterministic. A false `when` records a skipped `null` step.
`when` and `assert` must return a Boolean.

Any expanded call graph that may change state requires
`capabilities = ["mutate"]` on each calling workflow. An unsafe nested command
makes the exposed workflow unsafe to replay after reauthentication. Workflows
do not provide transactions or compensating rollback.

Use these read-only commands while authoring a pack:

```console
hubuum-cli extension validate ./inventory
hubuum-cli extension explain ./inventory
hubuum-cli extension explain ./inventory --workflow snapshot
```

TOML remains the declaration format; no second scripting language or external
interpreter is required. JQ provides bounded transformation, and built-in
commands provide effects. Work requiring external I/O or arbitrary recovery
belongs in an explicitly executable pack.

See the dependency-free
[`examples/hubuum-inventory`](../examples/hubuum-inventory/README.md) pack for a
complete example.

## Executable commands

The CLI validates the complete invocation before starting the process. It then
executes the declared executable directly, without a shell, followed by the
command's fixed `arguments` and the caller's argument vector. Host rendering
options such as `--output` and pipeline syntax are retained by Hubuum CLI and
are not forwarded.

Set `interactive = true` on a command that may prompt. Only a declared
interactive command inherits stdin, and only when Hubuum CLI itself has a
terminal. Other commands receive a closed stdin.

## Process protocol

The child receives these environment variables:

- `HUBUUM_EXTENSION_PROTOCOL=hubuum-cli.extension/v1`;
- `HUBUUM_EXTENSION_PACK` with the pack name;
- `HUBUUM_EXTENSION_CONFIG_JSON` with only the pack's namespaced configuration;
- `HUBUUM_CLI_BIN` with the current CLI executable, for trusted nested calls;
- non-secret server connection settings and, when configured, the token-file
  path.

Passwords and bearer token values are explicitly removed. A pack is a trusted
local program and is not sandboxed, so normal process privileges and readable
files still define its effective access.

The child writes diagnostics and prompts to stderr and exactly one tagged JSON
response to stdout. A successful response exits zero:

```json
{
  "protocol": "hubuum-cli.extension/v1",
  "status": "ok",
  "output": {
    "shape": "rows",
    "value": [{"name": "server-01", "jack": "J-42"}],
    "columns": ["name", "jack"]
  },
  "warnings": []
}
```

Supported shapes are `empty`, `lines`, `rows`, `detail`, `message`, and
`values`. Their JSON value must match the declared shape. Hubuum CLI validates
the response, applies its semantic pipeline, renders the requested text,
JSON, JSONL, CSV, or TSV output, and finally handles redirects.

An unsuccessful response exits nonzero and uses a stable snake-case code:

```json
{
  "protocol": "hubuum-cli.extension/v1",
  "status": "error",
  "error": {
    "code": "host_not_found",
    "message": "No Host matched server-01",
    "details": {"identifier": "server-01"}
  },
  "warnings": []
}
```

A mismatched exit status, unsupported protocol, malformed JSON, or invalid
semantic shape is reported as an extension protocol error.

## Lifecycle commands

The initial lifecycle accepts local package directories only. It does not use
an online registry, download code, verify signatures, or provide a sandbox.

```console
hubuum-cli extension list
hubuum-cli extension show site-inventory
hubuum-cli extension doctor
hubuum-cli extension validate ./site-inventory
hubuum-cli extension explain ./site-inventory --workflow snapshot
hubuum-cli extension reload

hubuum-cli extension install ./site-inventory
hubuum-cli extension upgrade ./site-inventory-v2
hubuum-cli extension disable site-inventory
hubuum-cli extension enable site-inventory
hubuum-cli extension remove site-inventory
```

Installation copies regular files and directories into the first configured
user root, rejects symlinks and special files, validates the staged copy, then
renames it into place. Upgrade requires an increasing semantic version unless
`--force` is supplied. System packs cannot be upgraded or removed by user
commands. Replaced and removed packages are moved under the user root's
`.trash` directory so recovery remains possible. Every mutation reloads
configuration and replaces the live command catalog; `extension reload` does
the same after an administrator changes files manually.

## Diagnostics

Use `extension doctor --output json` for automation. Diagnostics include a
stable code, severity, pack when known, source path, and actionable message.
The registry quarantines invalid manifests, incompatible CLI requirements,
missing or non-executable programs, and duplicate pack names. Quarantined packs
remain visible in `extension list` and `extension show` but register no commands.

For common failures:

- correct the path and executable mode reported by `executable_invalid`;
- update `requires_cli` or the installed CLI for `cli_incompatible`;
- remove or rename every conflicting copy for `duplicate_pack_name`;
- validate stdout independently when a command reports an extension protocol
  error, ensuring that prompts and logs go to stderr;
- run `extension reload` after manual package or configuration changes.

The Host wrapper pilot in
[`examples/hubuum-wrappers`](../examples/hubuum-wrappers/README.md) is a complete
v1 package that also preserves its standalone executables.
