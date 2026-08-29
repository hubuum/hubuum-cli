# Your first portable extension

This tutorial builds and installs a useful extension in about ten minutes. The
pack contains only `hubuum-extension.jsonc`; at runtime it relies on nothing but
`hubuum-cli`.

## 1. Generate a starter pack

From the repository root, create the read-only starter:

```sh
hubuum-cli extension init ./my-inventory --template read-only
```

The command refuses to overwrite a directory and validates its generated
workflow against the current CLI before writing it. Use `--name my-inventory`
when the directory name should not become the pack name.

The other templates are:

- `minimal`: one command and one built-in `run` step;
- `read-only`: typed configuration plus an inventory workflow;
- `executable`: a runnable process-protocol skeleton for cases that truly need
  an external program.

Portable templates are the default because they add no runtime dependency.

## 2. Read the whole manifest

Here is a complete minimal pack. JSONC permits comments and trailing commas,
but keys and strings still use JSON double quotes.

<!-- extension-manifest-example-start -->

```jsonc
{
  "$schema": "../schemas/hubuum-extension.schema.json",
  "schema_version": 1,
  "kind": "portable",
  "name": "my-inventory",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1.0",
  "config": {
    "objects_class": {
      "type": "string",
      "default": "Hosts",
      "help": "Class included in the inventory"
    }
  },
  "workflows": {
    "snapshot": {
      "output": { "shape": "rows", "type": "json" },
      "steps": [
        {
          "id": "objects",
          "kind": "run",
          "run": ["object", "list"],
          "with": {
            "class": { "config": "objects_class" },
            "all": true
          }
        }
      ],
      "result": ".steps.objects"
    }
  },
  "commands": {
    "snapshot": {
      "path": ["snapshot"],
      "workflow": "snapshot",
      "about": "List configured inventory objects"
    }
  }
}
```

<!-- extension-manifest-example-end -->

The hierarchy has three jobs:

1. `config` declares site settings and their types.
2. `workflows` declares inputs, ordered steps, and the final output contract.
3. `commands` chooses which workflows users can run and defines their CLI
   paths and options.

The private workflow name is `snapshot`. The installed command is
`extension my-inventory snapshot`.

## 3. Discover a built-in command contract

A `run` step calls a built-in command in-process. Do not guess the binding
names from its flags. Ask the CLI for the workflow-facing contract:

```sh
hubuum-cli extension contract object list
hubuum-cli extension contract object list --output json
hubuum-cli extension contract --list
```

For `object list`, the contract identifies `class` as a string, `all` as a
Boolean flag, and `where` as repeatable groups of three strings. It also says
whether the command may mutate data and whether authentication is required.
The keys under a step's `with` object must be these canonical input IDs.

Rendering options such as `output` and `table-headers` are deliberately absent.
They belong to the installed extension command, after the workflow produces its
semantic result.

## 4. Validate and explain

Run both authoring checks before installation:

```sh
hubuum-cli extension validate ./my-inventory
hubuum-cli extension explain ./my-inventory --workflow snapshot
```

`validate` checks the manifest, configuration defaults, built-in bindings,
types, call graph, mutation declarations, and limits. `explain` prints the
normalized `WorkflowPlan`: ordered steps, resolved bindings, effects, replay
safety, call depth, worst-case work, output declaration, and host limits.

Both commands are local and read-only. They do not require a Hubuum login and
do not execute workflow steps.

## 5. Configure, install, and run

Pack manifests are JSONC. Local Hubuum CLI configuration remains TOML. Override
this pack's default class in the normal CLI configuration:

```toml
[extensions.config.my-inventory]
objects_class = "Jacks"
```

Only this namespaced table is exposed to the pack, and only keys declared by
the manifest are accepted.

Install and invoke the pack:

```sh
hubuum-cli extension install ./my-inventory
hubuum-cli extension my-inventory snapshot
hubuum-cli extension my-inventory snapshot --output json
hubuum-cli extension my-inventory snapshot \
  \| JQ 'map(.name)' \
  \| L 10
```

The final two operators are the ordinary Hubuum CLI semantic pipeline. They run
after the workflow's declared `rows` output has been validated.

## 6. Add an input

Suppose users should choose the class per invocation. Add this to the workflow:

```jsonc
"inputs": {
  "class": { "type": "string", "required": true }
}
```

Change the run binding to:

```jsonc
"class": { "input": "class" }
```

Then add the exactly matching command option:

```jsonc
"options": {
  "class": {
    "kind": "string",
    "position": 1,
    "required": true,
    "help": "Class to list"
  }
}
```

Validate again. The command now reads:

```sh
hubuum-cli extension my-inventory snapshot Hosts
```

The workflow input and command option must match by declaration key, type,
required state, and repeatability. This duplication is intentional: workflows
remain reusable internal functions, while commands define a stable user-facing
interface.

## Where to continue

- [JSONC language reference](extension-reference.md) lists every field, shape,
  step, binding, limit, and naming rule.
- [Portable workflow recipes](extension-recipes.md) gives copyable patterns and
  an annotated `extension explain` result.
- [`examples/hubuum-recipes`](../examples/hubuum-recipes/README.md) is a small,
  compile-checked feature tour.
- [`examples/hubuum-placement`](../examples/hubuum-placement/README.md) is the
  complete dependency-free Host, Jack, Room, and move example.
