# Hubuum inventory workflow pack

This portable workflow pack composes three built-in `object list` commands from
one JSONC manifest without starting a shell or another `hubuum-cli` process. Its
only runtime dependency is `hubuum-cli`.

If this is your first pack, follow the
[ten-minute tutorial](../../docs/extension-tutorial.md) before changing this
example.

Install and run it with:

```console
hubuum-cli extension validate examples/hubuum-inventory
hubuum-cli extension explain examples/hubuum-inventory --workflow snapshot
hubuum-cli extension install examples/hubuum-inventory
hubuum-cli extension inventory snapshot --output json
hubuum-cli extension inventory classes --output json
```

The result is one object with `hosts`, `jacks`, and `rooms` arrays. Override
the default class names in the normal CLI configuration when needed:

```toml
[extensions.config.inventory]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
```

Every step is resolved and checked against the built-in command catalog's
workflow contract at load time, including input IDs, types, cardinality, and
command effects. The pack is explicitly portable and declares typed
configuration and output shapes; it contains no executable and needs nothing
but `hubuum-cli`. Commands that may change state require the manifest's explicit
`mutate` capability; this read-only example does not need it. Retry safety
remains a separate runtime property.
