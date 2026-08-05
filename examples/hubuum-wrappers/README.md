# Hubuum CLI wrappers

These example programs provide convenient Host inventory and placement
workflows without becoming part of the standard `hubuum-cli` distribution.
They are Bash wrappers and invoke `hubuum-cli` for every server operation; they
do not use a Hubuum API client library.

Keep the four files in this directory together and put the directory on `PATH`
if desired. The wrappers support the Bash 3.2 shipped by older macOS releases
and require `jq`, `mktemp`, and a working `hubuum-cli` configuration.
`hubuum-host-new` also uses the common `host` DNS utility unless `--no-dns` is
selected.

## Data and relation assumptions

The defaults match this model:

```text
Hosts <-> Jacks <-> Rooms
```

The class names can be changed with `HUBUUM_HOSTS_CLASS`,
`HUBUUM_JACKS_CLASS`, and `HUBUUM_ROOMS_CLASS`.

Host lookup accepts an exact object name or ID, plus these exact values under
`data.facts`:

- `identity.hostname`
- `identity.fqdn`
- `hardware.serial_number`
- `network.default_ipv4.address`
- `network.default_ipv6.address`
- `network.interfaces[*].mac_address`

Lookup is case-insensitive, ignores a trailing dot on DNS names, and accepts
colon- or hyphen-separated MAC addresses. Ambiguous values are rejected. Host
lookup first asks the server for object names containing the identifier. This
fast path covers the common short-hostname and FQDN cases. If those candidates
do not contain an exact supported identifier, the wrapper fetches the complete
class so serial, address, MAC, and other aliases can still be checked
consistently. Installations with very large classes may prefer a purpose-built
server-side lookup command for those fallback identifiers.

## Configuration

Normal `hubuum-cli` configuration files and `HUBUUM_CLI__...` environment
variables continue to work. Set `HUBUUM_CLI_BIN` when the executable is not
named `hubuum-cli` or is not on `PATH`.

An explicit collection can be configured for Host creation:

```sh
export HUBUUM_HOST_COLLECTION=inventory
```

`--collection` takes precedence over `HUBUUM_HOST_COLLECTION`. If neither is
set, `hubuum-host-new` runs `me permissions` and selects the collection only
when exactly one visible collection grants the current principal
`CreateObject`. It stops without creating anything when no collection or more
than one collection qualifies; the latter error lists the candidates so the
caller can choose explicitly.

Hubuum grants `CreateObject` on collections rather than on an individual
class. The inference is therefore deliberately collection-based: it does not
try writes or guess among multiple collections based on the `Hosts` class or a
token's narrower resource scope. The server remains authoritative for any
additional token restrictions when creation is attempted.

`hubuum-host-new` leaves a new Host unplaced unless `--to` is supplied. To use
an installation Jack as the default, set `HUBUUM_DEFAULT_JACK`; `--no-place`
overrides that environment variable.

The caller needs ordinary read permissions for the three classes and their
relations. Moving requires create and delete permission for object relations.
Creating requires create permission in the selected collection.

## `hubuum-host`

Show the current machine, a Host selected by any supported identifier, or all
raw data leaves:

```sh
hubuum-host
hubuum-host server-01.example.org
hubuum-host --id 00-11-22-33-44-55
hubuum-host --verbose server-01
hubuum-host --json server-01
```

The normal display selects useful fields from the facts structure and nests
Rooms below their Jacks. `--json` emits the resolved Host plus placement data.

## `hubuum-move`

A destination may be a Jack, Room, or Host. A Room with multiple Jacks prompts
for a choice; automation must select one with `--jack`. A Host destination uses
that Host's sole Jack. Use `--target-type` when the same name exists in more
than one class.

```sh
hubuum-move server-01 J-42
hubuum-move --from server-01 --to R-301 --jack J-42
hubuum-move server-01 server-02 --target-type host --mode switch
hubuum-move server-01 none
hubuum-move server-01 J-42 --dry-run
```

Moving removes the source Host's other Jack relations. When a target Jack is
occupied, `add` keeps its current occupants and `switch` exchanges Jacks with
one occupant. Interactive operation defaults to cancellation. Non-interactive
mutation requires `--yes`; occupied or multi-Jack choices still require
explicit `--mode`, `--swap-with`, or `--jack` values.

Hubuum does not expose a transaction spanning several relation commands. The
wrapper therefore validates the topology first and applies a small change
plan. If a later command fails, it attempts the inverse of every completed
command in reverse order and reports whether rollback was complete.

## `hubuum-host-new`

Create a Host with the supplied identity, serial, DNS addresses, source, schema
version, and collection timestamp under `data.facts`:

```sh
hubuum-host-new server-01.example.org SERIAL123  # Infer a unique collection
hubuum-host-new --collection inventory server-01.example.org SERIAL123
hubuum-host-new --collection inventory --to R-301 --jack J-42 server-01
hubuum-host-new --collection inventory --no-dns --ipv4 192.0.2.10 server-01
```

The object name defaults to the canonical FQDN and can be overridden with
`--name`. DNS failure stops creation unless `--no-dns` is explicitly selected.
Existing Hosts matched by name, identity, address, MAC, serial, or object ID are
never overwritten. Creation and placement are separate server operations; if
placement fails, the newly created Host is retained and reported as unplaced.

## Tests

Run the focused tests without contacting a Hubuum server:

```sh
examples/hubuum-wrappers/tests/test_wrappers.sh
```
