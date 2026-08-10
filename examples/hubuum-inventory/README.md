# Hubuum inventory workflow

This manifest-only extension composes three built-in `object list` commands
without starting a shell or another `hubuum-cli` process. Its only runtime
dependency is `hubuum-cli`.

Install and run it with:

```console
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
command effects. Commands that may change state require the manifest's
explicit `mutate` capability; this read-only example does not need it. Retry
safety remains a separate runtime property.
