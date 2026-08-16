# Extension packs

Extension packs add trusted, site-specific commands to Hubuum CLI without
compiling them into the binary. Their commands join normal help, REPL scopes,
option validation, completion, semantic pipelines, output formats, and
redirects under a reserved namespace:

```console
hubuum-cli extension placement host placement server-01
hubuum-cli extension placement room jacks R-301
```

## Choose a guide

- [Your first portable extension](extension-tutorial.md) is a ten-minute path
  from an empty directory to an installed, useful command.
- [Extension JSONC reference](extension-reference.md) lists every field, step,
  binding, output shape, naming rule, and mandatory limit.
- [Portable workflow recipes](extension-recipes.md) covers common composition
  patterns and annotates a real compiled plan.

The JSON Schema for editor validation and completion is
[`schemas/hubuum-extension.schema.json`](../schemas/hubuum-extension.schema.json).

## Portable or executable

| Kind | Implementation | Runtime dependencies | Use |
| --- | --- | --- | --- |
| `portable` | Typed JSONC workflows and bounded JQ | `hubuum-cli` only | Preferred for built-in command composition |
| `executable` | Program behind a versioned JSON protocol | Program and everything it needs | External I/O or behavior outside workflows |

Portable packs are deterministic and explainable. Their `run` steps invoke
built-in commands in-process; authors never construct a shell command or start
a second CLI process. Top-level workflows provide typed inputs and outputs,
same-pack reuse, assertions, conditions, and bounded iteration. The compiler
creates a stable `WorkflowPlan`, expands the call graph, rejects cycles, derives
effects and replay safety, and enforces fixed host limits.

JSONC is the sole workflow declaration language. JQ is its bounded expression
language. No external interpreter is required.

Executable packs remain useful when arbitrary code is the real requirement.
They are trusted local programs, not sandboxes, and their dependencies are an
installation concern. Keep a pack portable whenever built-in commands and JQ
can express it.

## Start and discover

Generate a pack whose manifest is already checked against this CLI:

```console
hubuum-cli extension init ./my-pack --template minimal
hubuum-cli extension init ./inventory --template read-only
hubuum-cli extension init ./external --template executable
```

Discover the typed contract available to a portable `run` step:

```console
hubuum-cli extension contract --list
hubuum-cli extension contract object list
hubuum-cli extension contract relation object create --output json
```

Contract details include canonical input IDs, types, cardinality, required
state, effects, authentication, and replay safety. Rendering options remain on
the outer extension command and are not workflow bindings.

Validate and inspect a local package without installing or executing it:

```console
hubuum-cli extension validate ./inventory
hubuum-cli extension explain ./inventory
hubuum-cli extension explain ./inventory --workflow snapshot
```

## Discovery and local configuration

The default system and user roots are `extensions.d` next to their corresponding
Hubuum CLI configuration files. Each immediate, non-hidden child directory is
one package containing `hubuum-extension.jsonc`. Missing roots are ignored.
Symlinked packages are ignored and diagnosed. Duplicate pack names quarantine
every copy instead of making precedence depend on discovery order.

Override roots and disable packs in the normal TOML application configuration:

```toml
[extensions]
system_roots = ["/opt/hubuum/extensions.d"]
user_roots = ["/home/alice/.config/hubuum/extensions.d"]
disabled = ["retired-pack"]

[extensions.config.placement]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
inventory_collection = "inventory"
relation_depth = 1
```

This is the only TOML involved: pack declarations themselves are JSONC. Only
the named `extensions.config.<pack>` table is resolved against that pack's
typed declarations. The complete application configuration is never exposed.
Extension roots and state remain local and are omitted from portable preference
export.

## Lifecycle

The current lifecycle works with local package directories. It does not use an
online registry, download code, verify signatures, or provide a sandbox.

```console
hubuum-cli extension install ./inventory
hubuum-cli extension list
hubuum-cli extension show inventory
hubuum-cli extension doctor
hubuum-cli extension upgrade ./inventory-v2
hubuum-cli extension disable inventory
hubuum-cli extension enable inventory
hubuum-cli extension remove inventory
hubuum-cli extension reload
```

Installation copies only regular files and directories, rejects symlinks and
special files, validates a staged copy, and renames it into the first user
root. Upgrade requires a higher semantic version unless `--force` is passed.
System packs cannot be upgraded or removed by user commands. Replaced and
removed user packages move under `.trash`, so the operation remains
recoverable. Mutations rebuild the live catalog; `reload` does the same after
manual administrator changes.

`extension doctor --output json` provides stable diagnostic codes for
automation. Invalid manifests, incompatible CLI requirements, configuration
errors, missing executables, compilation failures, and duplicate names
quarantine packs without registering their commands.

## Runtime and trust boundaries

Portable workflows can access only declared inputs, declared pack
configuration, earlier normalized step results, and step output metadata.
Cross-pack calls are prohibited. Mandatory bounds cover call depth, static and
runtime work, iteration, individual JQ evaluations, and cumulative output.
Mutating call graphs require an explicit `mutate` capability at every calling
boundary. They do not provide transactions or compensating rollback.

Executable children receive the protocol version, pack name, current CLI path,
non-secret server settings, token-file path when configured, and the pack's
namespaced configuration. Password and bearer token values are removed. A
trusted program can still read files and environment available to its normal
process identity, so review executable packs as code.

The program writes diagnostics to stderr and one tagged JSON response to
stdout. Hubuum CLI validates its status and semantic shape before applying the
ordinary pipeline, renderer, and redirect. See the
[executable reference](extension-reference.md#executable-packs) for the wire
format.

## Examples

All portable examples depend only on `hubuum-cli`:

- [`hubuum-inventory`](../examples/hubuum-inventory/README.md) is the smallest
  useful read-only pack.
- [`hubuum-jacks`](../examples/hubuum-jacks/README.md) introduces typed inputs
  and explicit step dependencies.
- [`hubuum-recipes`](../examples/hubuum-recipes/README.md) compile-checks every
  tagged step and binding form.
- [`hubuum-placement`](../examples/hubuum-placement/README.md) is one complete
  Host, Jack, Room, inventory, relation, and bounded move extension.

The legacy [`hubuum-wrappers`](../examples/hubuum-wrappers/README.md) pack is an
executable protocol and standalone-wrapper example.
