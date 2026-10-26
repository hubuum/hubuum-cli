# Hubuum CLI

<!-- markdownlint-disable-next-line MD033 -->
<span id="a-cli-for-hubuum"></span>

Run commands, explore interactively, or automate Hubuum from a terminal.
CLI **v0.0.22** (2026-10-26) uses Rust client **0.20.0** and targets server
**v0.0.21**. Hubuum is under active development before 1.0.

Discover fields in one bounded request, generate a schema suggestion, resume it
by task ID, and explicitly stage a reviewed candidate for impact analysis and
activation. The REPL uses the same inventory for field completion. See the
[schema workflow](docs/schema-evolution.md) and [release notes](CHANGELOG.md#v0-0-22).

Customize human-readable output by value type and edit colors with `theme set`.
Preview changes offline before saving per-theme foreground/background overrides.
See [themes and color editing](docs/themes.md).

Upgrade the server first. Field depth is limited to 0–32, and scripts should use
structured `Source`/`Path` identity because display labels can collide. In JSON,
JSONL, CSV, and TSV modes, warning and error text now goes to stderr; scripts
capturing diagnostics from stdout must change. Debug JSON retains its embedded
diagnostics. Earlier upgrades should also review the
[v0.0.20 migration notes](CHANGELOG.md#v0-0-20).

[Documentation](https://hubuum.github.io/hubuum-cli/) · [Hubuum ecosystem](https://hubuum.github.io/)

## Release binaries

Download and extract the archive for your platform from
[release v0.0.22](https://github.com/hubuum/hubuum-cli/releases/tag/v0.0.22).
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

CLI v0.0.22 uses client **0.20.0** and targets server **v0.0.21**.
Check the [compatibility matrix](COMPATIBILITY.md) for the server you use.
For server upgrades, follow the server's release instructions; for CLI recovery
commands, see [backup and restore](docs/backup-restore.md).

Password sessions persist in files by default; select `--session-storage none`
for memory-only use or opt into macOS Keychain/Linux Secret Service. Backup output remains format 10 and restore input supports formats 6–10. See
[authentication migration](docs/authentication.md).
**Breaking for wrapper-only upgrades:** upgrade `hubuum-room` together with
CLI v0.0.19 or newer; the wrapper requires `relation object graph --data`.

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
