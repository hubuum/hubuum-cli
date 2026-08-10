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

Every action is resolved and structurally validated through the complete
built-in command catalog at load time. Commands that are not safe to replay
are available with the manifest's explicit `allow_unsafe_actions` capability;
this read-only example does not need it.
