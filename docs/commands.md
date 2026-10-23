# Commands, scripts, and aliases

The Hosts, Rooms, and other specialist resources below are illustrative fixtures.
For an immediately loadable inventory, use [Atlas](example-dataset.md).

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

## Command diagnostics

Add `--debug` to a command to print its total duration and the client API
operations it performed. Each operation includes its duration and whether it
succeeded or failed:

```sh
hubuum-cli object list --class Hosts --debug
hubuum-cli --debug object list --class Hosts
hubuum-cli --debug script commands.hubuum
```

For example, the report for a successful list command might be:

```text
Debug: command took 18.750 ms; 2 client API calls; 2 HTTP attempts
  execute: 17.900 ms
    1. classes.get_by_name: 4.250 ms (ok)
      request GET /api/v1/classes/by-name/{class_name}: 4.200 ms (ok)
        HTTP GET attempt 1: 3.500 ms [status 200] (ok)
    2. QueryOp.page: 10.500 ms (ok)
      request GET /api/v1/classes/{class_id}/: 10.400 ms (ok)
        HTTP GET attempt 1: 9.700 ms [status 200] (ok)
  render: 0.800 ms
```

Text, JSONL, CSV, and TSV reports go to stderr. With `--output json` (or `--json`)
and `--debug`, the response is an object containing the original result in `data`
and structured diagnostics in `debug`:

```sh
hubuum-cli object list --class Hosts --output json --debug
```

```json
{
  "data": [],
  "debug": {
    "version": 1,
    "duration_ms": 18.75,
    "client_api_calls": 2,
    "http_attempts": 2,
    "custom_transport_attempts": 0,
    "client_library_spans": 0,
    "spans": []
  }
}
```

The example omits span entries for brevity. Each entry has `kind` (`stage`,
`client_call`, `client_library`, or `page`), `name`, `depth`, and `duration_ms`.
Client calls also have `succeeded`; supported native client spans have a `client`
object with applicable `method`, `route`, `transport`, `attempt`, `status`,
`outcome`, `page`, and `items` fields. Durations are milliseconds; `null` means a
span was unfinished or the entry is a page event. Spans are listed in start order,
with depth describing nesting under the command. `client_library_spans` counts
client spans outside the recognized diagnostics contract.

Warnings and errors appear as optional top-level `warnings` and `errors` arrays;
plain-text command messages appear in `messages`. Failed commands still exit
unsuccessfully. `data` is `null` when there is no semantic result; multiple
semantic results form an array. Pipelines transform the original result before
the wrapper is added. File redirects contain the same response; `each:` files
contain their individual result in `data` and the same whole-command report.
JSON output without `--debug` keeps its existing shape. Consumers enabling both
options must read result values from `.data` instead of the document root.
No duplicate human-readable debug report is printed to stderr in JSON mode.

The command option works in the REPL and scripts. Placing `--debug` before the
command enables it for every command in that invocation, including a whole
script or REPL session. A command-specific flag applies only to that command.

Total time includes command execution, pipeline processing, and result rendering
or file redirection for non-JSON reports. JSON diagnostics include result
preparation but exclude serialization and writing of the final response, which
contains those timings. Initial startup and login happen before command timing;
background polling and completion are excluded. Nested workflow operations share
the enclosing command's report.

The report is collected from `tracing` spans beneath the current command's root
span. Execution, rendering, and client calls emit their own spans. The context
follows command work onto blocking workers, and each script or REPL command starts
a fresh report. Detached spans and work that finishes after its command cannot
enter the next report. Timings measure elapsed wall time, including waits; nested
durations overlap and should not be added together. Debug collection works even
with `RUST_LOG=off`; the normal log filter applies independently.

Client 0.17.0 supplies versioned request, HTTP-attempt, retry, and stream spans,
plus automatic-pagination events. The report shows endpoint templates, response
statuses, retry attempts, pages, and outcomes using that documented contract.
Only those diagnostic fields are collected. Other descendant client spans appear
by name and lifetime, with their fields omitted.

Client API calls and HTTP attempts are counted separately: one API call can
perform several requests, including internal retries or automatic pagination.
HTTP-attempt timing ends at response headers; request timing also includes
buffered body processing. Redirects handled internally by the HTTP client remain
inside its send attempt. Custom transport invocations are counted separately
because they need not correspond to individual HTTP requests. Explicit page
iteration is recorded as `pages.next` for each page or failure.

For streaming calls, API-call timing ends when the client returns the stream;
the client body span follows consumption until EOF, failure, or cancellation.
Unfinished spans are marked as such when a command ends. Network activity inside
executable extensions is outside the CLI's client instrumentation. Operation
labels contain no argument values, credentials, response bodies, or error messages.

## Scripts and aliases

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

Larger site workflows can be installed as extension packs. They
live under the reserved `extension <pack> ...` namespace and join the normal
help tree, validation, completion, semantic output, pipeline, and redirect
machinery:

```sh
hubuum-cli extension init ./my-pack --template minimal
hubuum-cli extension contract object list
hubuum-cli extension validate examples/hubuum-placement
hubuum-cli extension explain examples/hubuum-placement
hubuum-cli extension install examples/hubuum-placement
hubuum-cli extension list
hubuum-cli extension placement host placement server-01
hubuum-cli extension placement room jacks R-301
hubuum-cli extension doctor
```

Portable workflow packs are the preferred extension kind. They run reusable,
typed JSONC workflows in-process, require no runtime dependency other than
`hubuum-cli`, and support bounded JQ expressions, conditions, assertions,
same-pack calls, and bounded iteration. Executable packs remain available for
work that cannot be expressed through built-in commands and JQ. They use a
small versioned JSON process protocol, may add runtime dependencies, and are
trusted rather than sandboxed. Start with the
[ten-minute extension tutorial](extension-tutorial.md), then use the
[extension overview](extensions.md),
[JSONC reference](extension-reference.md), and
[portable recipes](extension-recipes.md) for the complete model.
The [placement example](../examples/hubuum-placement/README.md) combines Host,
Jack, and Room operations in one dependency-free portable workflow pack.
The [Jacks example](../examples/hubuum-jacks/README.md) is a smaller introduction
to typed inputs and explicit step dependencies.
The [recipes example](../examples/hubuum-recipes/README.md) is a compile-checked
catalog of every tagged workflow step and binding form.

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
