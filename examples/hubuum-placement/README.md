# Hubuum placement workflow pack

This portable workflow pack combines the Host, Jack, and Room inventory
examples in one TOML manifest. It invokes built-in commands in-process and has
no runtime dependency other than `hubuum-cli`; it does not start a shell or an
external interpreter.

Validate, inspect, and install it with:

```console
hubuum-cli extension validate examples/hubuum-placement
hubuum-cli extension explain examples/hubuum-placement --workflow host_move
hubuum-cli extension install examples/hubuum-placement
```

The installed command tree is grouped by inventory type:

```console
hubuum-cli extension placement host list
hubuum-cli extension placement host show server-01
hubuum-cli extension placement host placement server-01

hubuum-cli extension placement jack list
hubuum-cli extension placement jack hosts J-42
hubuum-cli extension placement jack rooms J-42

hubuum-cli extension placement room list
hubuum-cli extension placement room jacks R-301
```

The extension also exposes typed creation and relation commands:

```console
hubuum-cli extension placement room create R-301
hubuum-cli extension placement jack create J-42
hubuum-cli extension placement host create server-01 \
  --data '{"facts":{"identity":{"hostname":"server-01"}}}'

hubuum-cli extension placement jack connect-room J-42 R-301
hubuum-cli extension placement host connect-jack server-01 J-42
```

## Host moves

`host move` composes relation lookup, bounded iteration, and same-pack workflow
calls. It removes every current Host-to-Jack relation and creates one relation
to the requested Jack. The command previews by default:

```console
hubuum-cli extension placement host move server-01 J-42
hubuum-cli extension placement host move server-01 J-42 --apply
```

The portable workflow runtime does not provide transactions or compensating
rollback. If an applied multi-step move fails partway through, completed
relation changes remain visible and the error identifies the completed steps.
Use `connect-jack` and `disconnect-jack` directly when an installation needs
manual control over each mutation.

Unlike the legacy shell wrappers, this deterministic example accepts exact
object names, does not perform DNS or identifier discovery, and never prompts.
Those constraints make the entire plan statically compilable and explainable.

## Configuration

The defaults model this topology:

```text
Hosts <-> Jacks <-> Rooms
```

Override class and collection names through the pack's typed configuration:

```toml
[extensions.config.placement]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
inventory_collection = "inventory"
relation_depth = 1
```

The pack demonstrates all compositional parts of the TOML workflow language:
private reusable workflows, typed inputs and configuration, declared output
shapes, `run`, `let`, `assert`, `call`, bounded `for_each`, `when`, mutating
capability propagation, and JQ result construction.
