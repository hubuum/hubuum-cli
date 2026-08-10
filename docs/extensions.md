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
option kinds are `string`, `integer`, `number`, `boolean`, and `flag`.
Positionals are ordered; only the final positional may be repeatable. The
`values` list provides validation and static completion.
One command path cannot be a prefix of another command path in the same pack.
Short and long option aliases share one namespace after removing their dashes,
and aliases reserved by host rendering options are rejected.

## In-process workflows

A workflow command declares sequential actions instead of an executable
argument vector. Each action resolves an existing built-in CLI command. Native
semantic output is consumed directly; commands that still render their JSON
are captured and parsed in-process, with non-JSON lines normalized to JSON
strings. The action values are composed into one detail object keyed by action
ID.

```toml
schema_version = 1
name = "inventory"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"

[[commands]]
path = ["snapshot"]
about = "Collect Hosts and Rooms"

[commands.workflow]

[[commands.workflow.actions]]
id = "hosts"
command = ["object", "list"]

[[commands.workflow.actions.arguments]]
literal = "--class"

[[commands.workflow.actions.arguments]]
config = "hosts_class"
default = "Hosts"

[[commands.workflow.actions.arguments]]
literal = "--all"

[[commands.workflow.actions]]
id = "rooms"
command = ["object", "list"]

[[commands.workflow.actions.arguments]]
literal = "--class"

[[commands.workflow.actions.arguments]]
config = "rooms_class"
default = "Rooms"

[[commands.workflow.actions.arguments]]
literal = "--all"

[[commands.workflow.actions]]
id = "first_host"
command = ["object", "show"]

[[commands.workflow.actions.arguments]]
literal = "--id"

[[commands.workflow.actions.arguments]]
action = "hosts"
selector = ".[0].id"
```

Action arguments are structured entries. `literal` supplies a fixed token,
`config` reads a scalar from `extensions.config.<pack>` and may declare a
`default`, and `option` forwards a required, non-repeatable workflow command
option by name. `action` reads an earlier action's JSON value; its optional
`selector` is an in-process JQ expression and must produce a value that can be
passed as one argument. Forward and recursive references are rejected. The CLI
never evaluates these tokens as a shell command.

Workflow packs need no `executable`. Every built-in catalog command except the
`extension` namespace can be an action. During catalog construction the CLI
validates command paths, fixed argument structure, required configuration,
output selectors, and replay safety. Invalid actions quarantine the pack and
appear in `extension doctor`.

Actions that are not safe to replay require an explicit workflow capability:

```toml
[commands.workflow]
allow_unsafe_actions = true
```

This includes commands that may change server or local state, plus conservative
commands that have not declared safe retry behavior. A workflow containing any
such action is itself never automatically replayed after reauthentication.
Completed action IDs are reported if a later action fails, but generic
workflows do not provide transactions or compensating rollback.

The runtime forces each action to internal JSON output, preserves warnings,
and then applies the caller's pipeline, output format, and redirect to the
composed result. Paged commands retain their normal JSON page metadata unless
the action supplies `--all`.

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
