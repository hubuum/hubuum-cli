# A CLI for Hubuum

This CLI interface for [Hubuum](https://github.com/hubuum/hubuum) is still in
pre-release state and under heavy development.

## Release binaries

Successful pushes to `main` publish rolling binaries in the
[`main-latest` release](https://github.com/hubuum/hubuum-cli/releases/tag/main-latest).
Version tags such as `v0.0.9` publish immutable, versioned GitHub releases.

Each release provides four small, stripped archives and matching SHA-256 files:

- Linux x86_64 and ARM64 binaries are statically linked with musl.
- The Apple Silicon macOS binary depends only on Apple-provided system libraries.
- The Windows x86_64 binary uses the MSVC ABI with a statically linked C runtime;
  Windows system DLLs remain platform dependencies.

Rolling builds identify their source commit using SemVer build metadata, for example
`v0.0.9+main.g0123456789ab`. Tagged releases use the clean package version. Show the
current build identity without logging in, or also query the configured server:

```sh
hubuum-cli version
hubuum-cli version --server
hubuum-cli version --output json
```

The same `version` commands are available in the REPL. The server version comes from
the server's unauthenticated OpenAPI metadata.

## Compatibility

CLI and server releases are versioned independently. The declared targets and
their client-library versions are recorded in the
[compatibility matrix](COMPATIBILITY.md). Hubuum CLI v0.0.9 targets Hubuum server
v0.0.9 through `hubuum_client` v0.9.0 and is the latest published release.

## Usage

Start the interactive REPL:

```sh
hubuum-cli
```

Run one command and exit:

```sh
hubuum-cli object list --limit 5
hubuum-cli collection list
hubuum-cli export list
hubuum-cli config paths
hubuum-cli help --tree
```

In a POSIX shell, quote or escape application-level pipe and redirect operators
so the shell passes them to Hubuum CLI as standalone arguments:

```sh
hubuum-cli config show \| F output \| L 5
hubuum-cli help \> help.txt
hubuum-cli config show \> each:/tmp/hubuum-config-{n}.txt
```

Operators do not need escaping inside the REPL or a Hubuum CLI script file.

Run commands from a script file:

```sh
hubuum-cli script commands.hubuum
```

Personal command aliases bind one root-level word to a complete command line,
including pipe stages and redirects. They are stored in the active user config
and participate in preference export/import. Built-in commands and scopes take
precedence over aliases.

```sh
hubuum-cli alias set --name hosts \
  --description 'List known hosts' \
  --command 'object list --class Hosts | P Name'
hubuum-cli hosts
hubuum-cli alias list
hubuum-cli alias show --name hosts
hubuum-cli alias unset --name hosts
```

`alias list`, root help, and `config show` use the optional description so long
command pipelines do not overwhelm summary output. `alias show` retains the
complete command. Described aliases use this compatible expanded TOML form;
existing `name = "command"` aliases remain valid:

```toml
[aliases.hosts]
command = "object list --class Hosts | P Name"
description = "List known hosts"
```

Larger site workflows can be installed as manifest-driven command packs. They
live under the reserved `extension <pack> ...` namespace and join the normal
help tree, validation, completion, semantic output, pipeline, and redirect
machinery:

```sh
hubuum-cli extension install ./site-inventory
hubuum-cli extension list
hubuum-cli extension site-inventory host show server-01
hubuum-cli extension doctor
```

Manifest-only packs run built-in CLI commands as in-process actions and require
no runtime dependency other than `hubuum-cli`. Executable-backed packs remain
available through a small versioned JSON process protocol; they are trusted,
not native plugins, and not sandboxed. See the [extension command pack
guide](docs/extensions.md) for manifests, discovery, configuration, lifecycle
operations, security boundaries, and troubleshooting.

Long aliases can be loaded from a one-command script file. This example finds
hosts whose kernel is older than the newest numeric kernel version observed in
the same OS major version:

```sh
hubuum-cli script examples/aliases/outdated-kernels.hubuum
hubuum-cli alias set --name outdated-kernels \
  --description 'Show hosts with kernels older than the newest observed for their OS release' \
  --command file://examples/aliases/outdated-kernels.hubuum
hubuum-cli outdated-kernels
```

The example converts each kernel into an array of numeric components, so
`553.16` becomes `[553, 16]` rather than `55316`. The example uses `--all` so
`object list` fetches the complete matching set before the local pipe runs.

`help`, `help --tree`, `version`, `config show`, and `config paths` run from the local
command catalog and configuration files without logging in. `version --server`,
`auth providers`, and `metrics` make unauthenticated requests. Other API-backed
commands authenticate before execution.

If an API-backed command receives `401 Unauthorized` in the interactive REPL,
Hubuum CLI reports that the session expired or the token was revoked, then
immediately renews the session. It rereads `--token-file` credentials, uses a
configured password without prompting, or prompts for the password when needed.
Read-only commands are retried once after a successful login. Commands that may
have changed server state are not replayed; the error identifies the first
failed HTTP method and path so the current state can be reviewed safely. One-shot
commands and scripts never start this interactive recovery flow.

Global configuration flags go before the command:

```sh
hubuum-cli --hostname api.example.com --username alice object list --limit 5
```

Discover identity providers before login, then select one for scoped credentials:

```sh
hubuum-cli --hostname api.example.com auth providers
hubuum-cli --hostname api.example.com --identity-scope corp-directory --username alice object list
hubuum-cli config set --key server.identity_scope --value corp-directory
```

For non-interactive automation, read a service-account bearer token from an
owner-only file. The token is not placed in the process arguments or copied into
the CLI token cache:

```sh
chmod 600 /run/secrets/hubuum.token
hubuum-cli --hostname api.example.com --token-file /run/secrets/hubuum.token object list --class Hosts
```

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
[Ansible fact publication guide](docs/ansible-facts.md) for the accepted JSON
Patch format, create-if-missing behavior, and service-account permissions.

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
`object fields`. This supplies escaped JSON Pointers for computed `--path`
options and dotted paths for aggregate dimensions, measures, and filters.
Inspected fields are cached for `cache.time` seconds (one hour by default) and
the cache can be bypassed with `cache.disable`.

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
flow. Backup documents may contain credential material, so backup and restore receipt
files are written with owner-only permissions on Unix and existing files require
`--force` before replacement:

```sh
hubuum-cli backup create --file hubuum-backup.json
hubuum-cli backup submit
hubuum-cli backup show 123
hubuum-cli backup download 123 --file hubuum-backup.json

hubuum-cli restore stage --file hubuum-backup.json --receipt restore-receipt.json
hubuum-cli restore status --receipt restore-receipt.json
hubuum-cli restore confirm --receipt restore-receipt.json --yes
```

Restore confirmation replaces all Hubuum data and invalidates existing bearer tokens.

For paginated commands, `--limit` requests a page size. The CLI currently
truncates values above 250 to the supported maximum with a
warning. Generated next-page commands retain that effective value. Paginated
commands also accept `--include-total` when an exact count is useful. Exact counts
can require additional server work, so they remain opt-in:

```sh
hubuum-cli object list --class Hosts --limit 25 --include-total
hubuum-cli task list --include-total --output json
```

Use `--all` to follow every remaining server cursor and buffer the complete
result before output pipelines run. `--limit` remains the page size when it is
combined with `--all`, and `--cursor <token> --all` starts from that cursor. The
CLI and client enforce automatic-pagination safety limits and reject repeated cursors.
Because complete results are held in memory, use `--all` deliberately for large
datasets:

```sh
hubuum-cli object list --class Hosts --all \| count
hubuum-cli audit list --cursor eyJpZCI6MTAwfQ --all --output json
```

If a pipeline is applied to a page that has more results without `--all`, the
CLI warns that the transformation only covered the current page.

Colored output defaults to terminal-aware `auto` mode and can be controlled per run or via `output.color`:

```sh
hubuum-cli --color never help
hubuum-cli --color always config paths
```

The current command vocabulary follows the Hubuum API:

- `collection` replaces the older namespace terminology.
- `export` replaces the older report terminology.
- `task list --kind export` filters export tasks.
- `task list --kind backup` filters backup tasks.
- `search --limit-per-kind` limits each result family independently.

Output pipes now support small in-process transformations in both the REPL and one-shot command mode.
The old shorthand still works:

```text
# before
object list --class Hosts | contact

# after
object list --class Hosts | grep contact | head 5
object list --class Hosts | reject retired | sort line desc | count
```

There are short aliases for the common DSL-shaped operations:

```text
object list --class Hosts | F contact | L 5 | C
object list --class Hosts | P name id | S !name
```

For shared table/detail output, pipes run against semantic JSON before rendering, so projection and field sorting affect every output format:

```text
config show | F output | P key value | S key
config show | VALUE key | C
config show | JQ 'map({key, value})' | L 5
object list --json --class Hosts | P Name os_version data.network.interfaces[*].ipv4
object list --class Hosts --computed S:average_load --computed P:note | F S:average_load>=1 | P Name S:average_load P:note | S S:average_load desc AS num
object show --class Hosts host-1 --computed S:average_load --computed P:note | P Name S:average_load P:note
```

Computed `S:<key>` and `P:<key>` fields are ordinary semantic selectors for
projection, filtering, sorting, grouping, aggregation, value extraction, and
redirection once selected with `--computed`. Their JSON number, boolean, object,
and array types are preserved through the pipe engine; computed errors remain
visible as `ERROR: ...` strings.
Top-level `--sort S:<key>` sorts the full matching set before `--limit`, while a
pipe sort operates on the rows returned by the object command.

See [docs/output-pipeline.md](docs/output-pipeline.md) for the semantic output pipeline direction.
See [docs/DSL.md](docs/DSL.md) for the full pipe DSL with Hubuum object examples.
See [docs/themes.md](docs/themes.md) for color themes, custom theme files, and palette licensing.
See [docs/manual-test.md](docs/manual-test.md) for a current manual smoke-test checklist.

Rendered output can be redirected to a file from the REPL, one-shot commands,
or scripts. These examples use REPL/script syntax:

```text
config show --output json > config.json
object list --class Hosts | P Name os_version > hosts.txt
object list --output jsonl --class Hosts | P Name data.network.interfaces[*].ipv4 >> hosts.jsonl
object list --json --class Hosts | P Name os_version > each:hosts/{Name}.json
object list --class Hosts | VALUE Name > each:names/{value}.txt
```

Use `>` to create or truncate the target file and `>>` to append. Operators
must be standalone, whitespace-delimited tokens. Redirect paths support
quoting, `~/...` expansion, and REPL file path completion. Parent directories
must already exist.

Use `each:<template>` to write one file per semantic row or value after pipe
stages have run; placeholders such as `{Name}`, `{data.owner}`, `{value}`, and
`{n}` can be used in the filename. A trailing redirect is accepted only when
the preceding command is valid. Compact pipeline comparisons such as
`F age>3` are therefore distinct from redirects, while command filters such as
`--where age > 3` continue to work normally.

Redirect files honor `output.color`: `auto` and `never` remove ANSI styling
from files, while `always` preserves it.

Machine-oriented output can be selected per command:

```sh
hubuum-cli config show --output json
hubuum-cli config show --output jsonl
hubuum-cli config show --output csv
hubuum-cli config show --output tsv
```

Table rendering can be tuned per run or with config keys:

```sh
hubuum-cli --table-style plain object list --limit 5
hubuum-cli --table-style dense --table-bands auto object list --limit 5
hubuum-cli --table-width full --table-wrap 40 object list --class Hosts
hubuum-cli --empty-result silent object list --class Hosts --limit 0
hubuum-cli object list --class Hosts --table-headers full
hubuum-cli object list --class Hosts --table-headers none
```

Grouped headers are the default for text tables. Dotted paths are displayed on
multiple header lines so path names do not determine individual column widths;
unambiguous class aliases take precedence. Use `--table-headers full` for the
original flat paths, or `--table-headers none` to suppress table headers.
Machine-oriented formats retain their semantic column names. Persist the mode
with `hubuum-cli config set --key output.table_headers --value none`.

Related config keys are `output.table_style`, `output.table_headers`,
`output.table_width`, `output.table_wrap`, `output.table_bands`, and
`output.empty_result`.

Large payload options can read from explicit value sources. This is opt-in per option, so ordinary values such as remote target URLs remain literal.

```sh
hubuum-cli object create --name item-1 --class Device --collection main --description "imported" --data file://payload.json
hubuum-cli class create --name Device --collection main --description "devices" --schema https://example.com/schema.json
```
