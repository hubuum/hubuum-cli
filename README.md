# Hubuum CLI

<!-- markdownlint-disable-next-line MD033 -->
<span id="a-cli-for-hubuum"></span>

Run commands, explore interactively, or automate Hubuum from a terminal.
CLI **v0.0.14** (2026-10-06) uses Rust client **0.14.1** and targets server
**v0.0.18**. Hubuum is under active development before 1.0.

[Documentation](https://hubuum.github.io/hubuum-cli/) · [Hubuum ecosystem](https://hubuum.github.io/)

## Release binaries

Download and extract the archive for your platform from
[release v0.0.14](https://github.com/hubuum/hubuum-cli/releases/tag/v0.0.14).
Verify its published SHA-256 checksum and place `hubuum-cli` (`hubuum-cli.exe`
on Windows) on your `PATH`. Linux x86_64/ARM64, macOS Apple Silicon, and Windows
x86_64 binaries are available. See [binary details](docs/updates.md).

```sh
hubuum-cli version
```

### Updating in place

Use `hubuum-cli self-update --check` to inspect the latest stable release.
See [update and restart instructions](docs/updates.md#updating-in-place) before
installing it, including package-manager and platform restrictions.

## Compatibility

Check the [compatibility matrix](COMPATIBILITY.md) for the server you use.
For server upgrades, follow the server's release instructions; for CLI recovery
commands, see [backup and restore](docs/backup-restore.md).

## Usage

You need a server URL and a human account with permission to read its data.
Connect to a TLS-enabled server; the CLI prompts for the password:

```sh
hubuum-cli --hostname hubuum.example.com --protocol https --port 443 \
  --username alice collection list
```

For a local HTTP evaluation server, use `--hostname 127.0.0.1 --protocol http
--port 8080`. Global options go before the command. See
[authentication](docs/authentication.md) for provider scopes, token files, and
session recovery.

After loading [Atlas](docs/example-dataset.md) and granting your account read
access, list its three Server objects:

```sh
hubuum-cli --hostname hubuum.example.com --protocol https --port 443 \
  --username alice object list --class Server
```

Expect web-01, web-02, and worker-01. To keep a session open, omit the command
and use the interactive REPL. `help --tree` lists available commands.

| Next task | Guide |
| --- | --- |
| Run scripts and define aliases | [Commands and scripts](docs/commands.md) |
| Search inventory | [Search](docs/search.md) |
| Patch data or use computed fields | [Object workflows](docs/object-workflows.md) |
| Page through results | [Pagination](docs/pagination.md) |
| Transform or save output | [Output pipelines](docs/output-pipeline.md) |
| Add site-specific commands | [Extension tutorial](docs/extension-tutorial.md) |
| Change colors | [Themes](docs/themes.md) |

## Schema evolution and cancellation

See [schema evolution](docs/schema-evolution.md) and [task discovery](docs/tasks.md).

## Documentation-only CI

See [documentation maintenance](docs/documentation.md#documentation-only-ci).
