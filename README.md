# A CLI for Hubuum

This CLI interface against [Hubuum](https://github.com/hubuum/hubuum) is still in pre-release state and under heavy development.

## Usage

Start the interactive REPL:

```sh
hubuum-cli
```

Run one command and exit:

```sh
hubuum-cli object list --limit 5
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
```

See [docs/output-pipeline.md](docs/output-pipeline.md) for the semantic output pipeline direction.

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
hubuum-cli object create --name item-1 --class Device --namespace main --description "imported" --data file://payload.json
hubuum-cli class create --name Device --namespace main --description "devices" --schema https://example.com/schema.json
```
