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

Large payload options can read from explicit value sources. This is opt-in per option, so ordinary values such as remote target URLs remain literal.

```sh
hubuum-cli object create --name item-1 --class Device --namespace main --description "imported" --data file://payload.json
hubuum-cli class create --name Device --namespace main --description "devices" --schema https://example.com/schema.json
```
