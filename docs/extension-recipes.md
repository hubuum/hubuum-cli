# Portable workflow recipes

These patterns assume a portable pack, so they execute in-process and depend
only on `hubuum-cli`. Copy the complete, compile-checked versions from
[`examples/hubuum-recipes`](../examples/hubuum-recipes/README.md).

## Find the right bindings for a `run` step

A **command contract** is the CLI's description of how a workflow may call one
of its built-in commands. It names the keys accepted by the step's `with`
object and describes each key's value type, whether it is required, and whether
it accepts one value or a repeated group. The contract also reports operational
properties such as whether the command needs authentication or may change data.

`contract --list` lists the built-ins that a `run` step may call. Pass one of
those command paths to inspect its contract:

```sh
hubuum-cli extension contract --list
hubuum-cli extension contract object list
```

The second command reports that `where` has `repeated_fixed` cardinality with
groups of three. Bind one clause as a flat array:

```jsonc
{
  "id": "servers",
  "kind": "run",
  "run": ["object", "list"],
  "with": {
    "class": "Hosts",
    "where": ["name", "contains", "server"],
    "all": true
  }
}
```

## Pass typed values

Use the source that owns the value:

```jsonc
"with": {
  "from_user": { "input": "class" },
  "from_site": { "config": "hosts_class" },
  "from_step": { "step": "classes" },
  "one_field": { "step": "classes", "select": ".[0].name" },
  "plain_text": "Hosts",
  "object_data": { "literal": { "site": "oslo" } }
}
```

The keys on the left must exist on the called command or workflow. The example
uses descriptive placeholders; a real `run` uses IDs from `extension contract`.

## Reuse a private workflow

Declare a workflow without a corresponding command, then call it:

```jsonc
{
  "id": "hosts",
  "kind": "call",
  "call": "list_one",
  "with": { "class": "Hosts" }
}
```

Calls stay in the current pack. Input names, types, and repeatability are
checked statically. The compiler rejects missing targets and call cycles.

## Iterate with a hard bound

The target workflow declares an input matching `as`:

```jsonc
{
  "id": "objects_by_class",
  "kind": "for_each",
  "items": { "input": "classes" },
  "as": "class",
  "call": "list_one",
  "max_items": 10,
  "when": ".input.enabled"
}
```

`classes` must resolve to an array. Runtime length must be no more than both
`max_items` and the host limit. Results preserve input order.

## Transform and check values

Use `let` for a bounded transformation and `assert` for an invariant:

```jsonc
[
  {
    "id": "names",
    "kind": "let",
    "expr": ".steps.hosts | map(.name)"
  },
  {
    "id": "has_hosts",
    "kind": "assert",
    "condition": "(.steps.names | length) > 0",
    "message": "No Hosts were found"
  }
]
```

An assertion condition and a `when` expression must yield exactly one Boolean.

## Declare repeatable command input

The workflow input:

```jsonc
"classes": {
  "type": "string",
  "repeatable": true,
  "default": ["Hosts", "Jacks"]
}
```

must match its public command option:

```jsonc
"classes": {
  "kind": "string",
  "long": "class",
  "repeatable": true,
  "help": "Class to visit; may be repeated"
}
```

Users can then pass `--class Hosts --class Jacks`.

## Safely expose mutation

First inspect the built-in target:

```sh
hubuum-cli extension contract relation object create
```

Any workflow whose expanded calls may mutate data declares the capability:

```jsonc
"capabilities": ["mutate"]
```

Every calling workflow must make this acknowledgement. The compiler propagates
mutation and replay safety through `call` and `for_each`. This is an effects
declaration, not a transaction: completed writes are not rolled back after a
later failure.

## Read an explain result

Run:

```sh
hubuum-cli extension explain examples/hubuum-recipes --workflow tour \
  --output json
```

The workflow entry at `.plan.workflows[0]` contains these sections (irrelevant
fields are omitted here):

```json
{
  "name": "tour",
  "effects": "read_only",
  "requires_authentication": true,
  "reauthentication_retry": "safe",
  "call_depth": 2,
  "worst_case_operations": 38,
  "output": {
    "shape": "detail",
    "type": "json"
  },
  "steps": [
    {
      "id": "configured",
      "kind": "run",
      "run": "object list",
      "with": {
        "class": { "config": "fallback_class" }
      }
    }
  ]
}
```

- `effects` is expanded across nested calls, not copied from the outer step.
- `requires_authentication` says execution needs a configured server session;
  validate and explain themselves remain offline.
- `reauthentication_retry` describes whether the whole exposed command can be
  replayed safely after renewing a session.
- `call_depth` and `worst_case_operations` are compiler results checked against
  the adjacent `limits` object.
- `steps` is the stable `WorkflowPlan`, with binding sources made explicit.

Use `--workflow` to keep a large pack readable. Without it, `explain` returns
all public and private workflow plans.

## Debug validation failures

Use this order:

1. Let the editor schema catch misspelled fields and wrong JSON shapes.
2. Run `extension contract` for an unknown `run` binding or wrong cardinality.
3. Run `extension validate` for the complete cross-reference and type error.
4. Run `extension explain --workflow NAME` to verify effects, conditions,
   normalized bindings, call expansion, and limits.
5. For installed discovery failures, run `extension doctor --output json`.

Forward step references, cross-pack calls, cycles, undeclared mutation,
unbounded iteration, wrong output shapes, and incompatible command interfaces
are all rejected before a portable command is registered.
