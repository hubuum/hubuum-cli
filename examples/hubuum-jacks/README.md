# Hubuum Jacks workflow pack

This small portable pack shows how a JSONC manifest keeps workflows, ordered
steps, bindings, and their public commands visibly nested. It lists Jacks and
looks up the Hosts or Rooms related to one Jack. Every operation runs through
the built-in command catalog, so the pack needs only `hubuum-cli`.

Validate, inspect, and install it with:

```console
hubuum-cli extension validate examples/hubuum-jacks
hubuum-cli extension explain examples/hubuum-jacks --workflow jack_hosts
hubuum-cli extension install examples/hubuum-jacks
hubuum-cli extension jacks list
hubuum-cli extension jacks hosts J-42
hubuum-cli extension jacks rooms J-42
```

Override the class names or relation depth through typed pack configuration:

```toml
[extensions.config.jacks]
hosts_class = "Hosts"
jacks_class = "Jacks"
rooms_class = "Rooms"
relation_depth = 1
```

The manifest intentionally uses JSONC comments and a trailing comma while
otherwise retaining strict JSON syntax.
