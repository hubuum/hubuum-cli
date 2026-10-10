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
