# A CLI for Hubuum

This CLI interface for [Hubuum](https://github.com/hubuum/hubuum) is still in
pre-release state and under heavy development.

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

`help`, `help --tree`, `config show`, and `config paths` run from the local command catalog and configuration files without logging in. API-backed commands authenticate before execution.

Global configuration flags go before the command:

```sh
hubuum-cli --hostname api.example.com --username alice object list --limit 5
```

Colored output defaults to terminal-aware `auto` mode and can be controlled per run or via `output.color`:

```sh
hubuum-cli --color never help
hubuum-cli --color always config paths
```

The current command vocabulary follows the Hubuum API:

- `collection` replaces the older namespace terminology.
- `export` replaces the older report terminology.
- `task list --kind export` filters export tasks.
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
```

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
```

Related config keys are `output.table_style`, `output.table_width`, `output.table_wrap`, `output.table_bands`, and `output.empty_result`.

Large payload options can read from explicit value sources. This is opt-in per option, so ordinary values such as remote target URLs remain literal.

```sh
hubuum-cli object create --name item-1 --class Device --collection main --description "imported" --data file://payload.json
hubuum-cli class create --name Device --collection main --description "devices" --schema https://example.com/schema.json
```
