# Hubuum inventory workflow

This manifest-only extension composes three built-in `object list` commands
without starting a shell or another `hubuum-cli` process. Its only runtime
dependency is `hubuum-cli`.

Install and run it with:

```console
hubuum-cli extension install examples/hubuum-inventory
hubuum-cli extension inventory snapshot --output json
```

The result is one object with `hosts`, `jacks`, and `rooms` arrays. Override
the default class names in the normal CLI configuration when needed:

```toml
[extensions.config.inventory]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
```

Every action is resolved through the built-in command catalog at load time.
Only commands explicitly marked as composable and read-only can be used.
