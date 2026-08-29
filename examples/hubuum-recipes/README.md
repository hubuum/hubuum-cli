# Portable workflow recipes

This small pack is a compile-checked catalog of all five portable step kinds
and every binding form. It depends only on `hubuum-cli`.

Validate it and inspect its normalized plan before reading individual steps:

```sh
hubuum-cli extension validate examples/hubuum-recipes
hubuum-cli extension explain examples/hubuum-recipes --workflow tour
```

The `tour` workflow demonstrates literal scalar and array bindings, an explicit
literal object, typed input and configuration bindings, an earlier-step binding
with `select`, a fixed-arity `where` binding, repeatable input, and `when`. Its
private workflows demonstrate reusable `call` and bounded `for_each` targets.

Run it against a configured Hubuum server:

```sh
hubuum-cli extension recipes tour --class Hosts --class Jacks --output json
```

For a minimal first pack, see
[`hubuum-inventory`](../hubuum-inventory/README.md). For a realistic Host, Jack,
Room, and move system, see
[`hubuum-placement`](../hubuum-placement/README.md).
