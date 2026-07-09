# Manual Test Checklist

This checklist targets the current Hubuum CLI command surface and a current
Hubuum server using `hubuum_client` 0.2.x. It intentionally uses the current
terms `collection` and `export`; old `namespace` and `report` commands are not
kept for compatibility.

## Setup

Use a test server and an account that can create temporary collections,
classes, objects, event resources, exports, imports, and remote targets.

```sh
hubuum-cli --hostname hubuum.math.uiocloud.no --protocol https --port 443 --username admin help --tree
```

For repeated testing, start the REPL with the same connection flags:

```sh
hubuum-cli --hostname hubuum.math.uiocloud.no --protocol https --port 443 --username admin
```

Local commands that should not require login:

```text
help
help --tree
help pipe
help shell
config paths
config show
theme list
```

## Collections, Classes, And Objects

Create a temporary collection:

```text
collection create --name cli-smoke --description "CLI smoke collection" --owner admins
collection list --where name contains cli-smoke
collection show cli-smoke
collection modify cli-smoke --description "CLI smoke collection updated"
```

Create and inspect a class:

```text
class create --name SmokeHost --collection cli-smoke --description "Smoke hosts"
class list --where collection = cli-smoke
class show SmokeHost
class modify SmokeHost --description "Smoke hosts updated"
```

Create, inspect, and update an object:

```text
object create --name smoke-1 --class SmokeHost --collection cli-smoke --description "Smoke object" --data '{"os_version":"15.7.7","owner":"ops","network":{"interfaces":[{"ipv4":"129.240.1.10"}]}}'
object list --class SmokeHost --limit 10
object show --class SmokeHost smoke-1
object modify --class SmokeHost smoke-1 --description "Smoke object updated" --data owner=platform
object fields --class SmokeHost
```

Expected results:

- Commands render tables or details with collection names, not namespace names.
- `--limit 10` returns at most ten hits and does not by itself imply a follow-up page.
- A pagination prompt appears only when the server returns a cursor.

## Pipe DSL And Redirects

Run semantic pipeline checks against real object output:

```text
object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4
object list --class Hosts | F os_version contains 26
object list --class Hosts | V 129.240
object list --class Hosts | K ipv4
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num | L 10
object list --class Hosts | VALUE Name | C
```

Run redirect checks in a temporary directory:

```text
config show --output json > /tmp/hubuum-config.json
object list --class Hosts | P Name os_version > /tmp/hubuum-hosts.txt
object list --class Hosts | VALUE Name > each:/tmp/hubuum-host-names/{value}.txt
```

Expected results:

- Pipes operate on structured output, not rendered table glyphs.
- Grouped aggregate output suppresses cursor pagination prompts after terminal grouping stages.
- `>` truncates, `>>` appends, and `each:<template>` creates one file per semantic row or value.
- Field placeholders in `each:<template>` are sanitized before writing.

## Exports

List and inspect export templates:

```text
export list
export list --where name contains smoke --sort name asc --limit 10
export show <template-name>
```

Create, run, and remove a simple export template:

```text
export create --name cli-smoke-export --collection cli-smoke --description "Smoke export" --content-type text/plain --template "Export {{ scope.kind }}"
export run --template cli-smoke-export --scope collections --wait --timeout 60
task list --kind export --limit 5
task output <task-id>
export delete cli-smoke-export
```

Run an export without a template:

```text
export run --scope objects_in_class --class Hosts --query "os_version contains 26" --max-items 10 --wait --timeout 60
```

Expected results:

- Export task output is fetched through `task output` or `jobs output`.
- `task list --kind export` accepts `export`; `report` should be rejected.
- `help report` should return `Command not found: report`.

## Imports

Submit import JSON from a file or HTTP body source:

```text
import submit --file /tmp/hubuum-import.json --collection cli-smoke --collision-policy overwrite --wait --timeout 120
import show <task-id>
import results <task-id>
```

Expected results:

- `--collection` rewrites import collection references to an existing collection.
- Policy flags override the mode in the import request body.
- Import results can be listed and sorted with `--sort`.

## Search

Run unified search checks:

```text
search root --kind collection --limit-per-kind 1
search --query Hosts --kind class --kind object --limit-per-kind 5
search smoke --stream --kind class --kind object --search-object-data
```

Expected results:

- `--limit-per-kind` limits each result family independently.
- Cursor output uses `--cursor-collections`, `--cursor-classes`, and `--cursor-objects`.
- The old `--limit` search option should be rejected with a useful suggestion.

## Relations

Create a second class and class relation:

```text
class create --name SmokeService --collection cli-smoke --description "Smoke services"
relation class create --class-a SmokeHost --class-b SmokeService
relation class list --root-class SmokeHost
relation class direct --root-class SmokeHost
relation class graph --root-class SmokeHost --max-depth 2
```

Create and inspect an object relation:

```text
object create --name service-1 --class SmokeService --collection cli-smoke --description "Smoke service" --data '{"tier":"test"}'
relation object create --class-a SmokeHost --object-a smoke-1 --class-b SmokeService --object-b service-1
relation object list --root-class SmokeHost --root-object smoke-1
relation object direct --root-class SmokeHost --root-object smoke-1
relation object graph --root-class SmokeHost --root-object smoke-1 --max-depth 2
```

Expected results:

- Relation list, direct, and graph views resolve class and object names.
- Class/object `show` includes relation summaries.

## Tasks And Background Jobs

Submit a background task and inspect it through both job aliases:

```text
export run --scope collections
jobs list
jobs show <local-job-id>
jobs watch <local-job-id>
jobs output <local-job-id>
bg list
bg forget <local-job-id>
```

Inspect server tasks directly:

```text
task queue
task list --kind export --status succeeded --limit 5
task show <task-id>
task events <task-id> --sort created_at desc
task output <task-id>
```

Expected results:

- `jobs` and `bg` aliases behave the same.
- `task output` renders export output and import result summaries.
- Remote-call task output may be unavailable if the server/client endpoint does not expose it.

## IAM And Permissions

Check identity and token commands:

```text
whoami
me show
me groups
me permissions
me tokens
```

Check collection permissions:

```text
collection permissions list cli-smoke
collection permissions set cli-smoke --group admins --ReadCollection --ReadClass --ReadObject
collection principal-permissions cli-smoke --principal-id <principal-id>
```

Check user, group, and service account command help and list output:

```text
user list --limit 5
group list --limit 5
service-account list --limit 5
```

Expected results:

- Permission command names use `collection`.
- User rename is rejected explicitly if the server/client model does not expose it.
- Token create/list/revoke commands work for supported principals.

## Events And Remote Targets

Smoke event infrastructure:

```text
event-sink list
event-subscription list
event-delivery health
event-delivery list --limit 5
audit list --limit 5
audit show --id <audit-event-id>
history class SmokeHost
history object --class SmokeHost smoke-1
```

Smoke remote targets if a safe endpoint is available:

```text
remote-target list
remote-target create --name cli-smoke-target --collection cli-smoke --description "Smoke target" --url https://example.com --method get --subject-types collection,class,object --auth-type none
remote-target show cli-smoke-target
remote-target invoke cli-smoke-target --subject collection --collection cli-smoke --wait --timeout 60
remote-target delete cli-smoke-target
```

Expected results:

- Event and audit commands render current resource names.
- Remote-target subject options use `collection`, `class`, `object`, `class_relation`, and `object_relation`.

## Themes, Tables, And Help

Check runtime ergonomics:

```text
theme list
theme preview catppuccin-mocha
theme use solarized-dark
config show | P key value | F output.theme
```

Check output controls:

```text
object list --class Hosts --limit 5 --table-style dense --table-bands auto
object list --class Hosts --limit 5 --table-width full --table-wrap 40
object list --class Hosts --limit 0 --empty-result silent
```

Check focused help:

```text
help collection
help export
help pipe group
help pipe redirects
help shell completion
```

Expected results:

- Help text colors only command fragments when color is enabled.
- Dense table bands are subtle on dark backgrounds.
- Theme selection works at runtime and persists through config when requested.

## Cleanup

Remove temporary resources in dependency order:

```text
relation object delete --class-a SmokeHost --object-a smoke-1 --class-b SmokeService --object-b service-1
relation class delete --class-a SmokeHost --class-b SmokeService
object delete --class SmokeService --name service-1
object delete --class SmokeHost --name smoke-1
class delete SmokeService
class delete SmokeHost
collection delete cli-smoke
```

If a cleanup step fails because a resource was not created or was already
removed, continue with the remaining cleanup commands.
