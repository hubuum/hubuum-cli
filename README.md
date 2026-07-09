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
object list --json --class Hosts | P Name os_version data.network.interfaces[*].ipv4
```

See [docs/output-pipeline.md](docs/output-pipeline.md) for the semantic output pipeline direction.
See [docs/DSL.md](docs/DSL.md) for the full pipe DSL with Hubuum object examples.
See [docs/themes.md](docs/themes.md) for color themes, custom theme files, and palette licensing.
See [docs/manual-test.md](docs/manual-test.md) for a current manual smoke-test checklist.

Rendered output can be redirected to a file from the REPL, one-shot commands, or scripts:

```text
config show --output json > config.json
object list --class Hosts | P Name os_version > hosts.txt
object list --json --class Hosts | P Name data.network.interfaces[*].ipv4 >> hosts.json
object list --json --class Hosts | P Name os_version > each:hosts/{Name}.json
object list --class Hosts | VALUE Name > each:names/{value}.txt
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts
```

Use `>` to create or truncate the target file and `>>` to append. Redirect paths support quoting, `~/...` expansion, and REPL file path completion. Use `each:<template>` to write one file per semantic row or value after pipe stages have run; placeholders such as `{Name}`, `{data.owner}`, `{value}`, and `{n}` can be used in the filename. A trailing redirect is parsed only when the command before it is valid, so filter operators such as `--where age > 3` still work normally.

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
