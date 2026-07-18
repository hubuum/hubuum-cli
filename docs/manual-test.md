# Manual Test Checklist

This checklist targets the current Hubuum CLI command surface and Hubuum server
v0.0.2 using `hubuum_client` 0.5.x. It intentionally uses the current
terms `collection` and `export`; old `namespace` and `report` commands are not
kept for compatibility.

## Setup

Use a test server and an account that can create temporary collections,
classes, objects, event resources, exports, imports, and remote targets.

```sh
hubuum-cli --hostname hubuum.example.org --protocol https --port 443 --username admin help --tree
```

For repeated testing, start the REPL with the same connection flags:

```sh
hubuum-cli --hostname hubuum.example.org --protocol https --port 443 --username admin
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
metrics
metrics --path /internal/metrics
```

The metrics commands make unauthenticated requests. The default command should
return Prometheus exposition text from `/metrics`; test `--path` only when that
alternate route is configured on the server. The server's client allowlist still
applies.

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
object list --class SmokeHost --limit 500
object show --class SmokeHost smoke-1
object modify --class SmokeHost smoke-1 --description "Smoke object updated" --data owner=platform
object fields --class SmokeHost
```

Create shared and personal computed definitions, preview them, and verify
computed object reads are selected explicitly:

```text
computed shared create --class SmokeHost --key owner_copy --label "Owner copy" --operation first_non_null --path /owner --result-type string
computed shared list --class SmokeHost
computed shared preview --class SmokeHost --key owner_copy --label "Owner copy" --operation first_non_null --path /owner --result-type string --object smoke-1
computed shared rebuild --class SmokeHost
computed personal create --class SmokeHost --key owner_personal --label "Personal owner" --operation first_non_null --path /owner --result-type string
computed personal list --class SmokeHost
object show --class SmokeHost smoke-1 --computed S:owner_copy
object list --class SmokeHost --computed all --output json
object list --class SmokeHost
object list --class SmokeHost --computed S:owner_copy --computed P:owner_personal
object list --class SmokeHost --sort S:owner_copy asc --limit 10
object list --class SmokeHost --sort P:owner_personal desc --limit 10
object list --class SmokeHost --computed S:owner_copy --computed P:owner_personal | F S:owner_copy ops | P Name S:owner_copy P:owner_personal
object show --class SmokeHost smoke-1 --computed S:owner_copy --computed P:owner_personal | P Name S:owner_copy P:owner_personal
```

In the REPL, type `computed shared create --class SmokeHost --path /` and press
Tab. Verify that schema paths are offered when `SmokeHost` has a JSON Schema.
Repeat with a schema-less class containing objects and verify that observed data
paths from the first 100 sampled objects are offered as JSON Pointers.

Type `object list --class SmokeHost --sort S:` and press Tab. Verify enabled
shared definitions are offered; repeat with `P:` for personal definitions.
Repeat after `--computed` and verify `all`, `none`, `S:<key>`, and `P:<key>` are
offered.
Configure defaults for the smoke class and verify they apply to both list and
show, then verify an explicit selection replaces them:

```text
config set --key output.object_class_computed_fields.SmokeHost --value S:owner_copy,P:owner_personal
object list --class SmokeHost
object show --class SmokeHost smoke-1
object list --class SmokeHost --computed none
object show --class SmokeHost smoke-1 --computed S:owner_copy
config unset --key output.object_class_computed_fields.SmokeHost
```

Configure a local display alias and verify both the canonical and legacy names
load it:

```toml
[output.object_list_class_aliases.SmokeHost]
owner_display = ["data.owner", "data.contact.owner"]
```

Expected results:

- Commands render tables or details with collection names, not namespace names.
- `--limit 10` requests a page size and is sent unchanged.
- A value above the server v0.0.2 maximum, such as `--limit 500`, produces a
  warning, sends 250, and preserves `--limit 250` in the generated next-page command.
- Object reads omit computed values unless selected by a per-class default or by
  `--computed S:<key>`, `--computed P:<key>`, or `--computed all`.
- Individual computed selections exclude unselected values; `--computed all`
  retains every available computed value.
- `output.object_class_computed_fields.<class>` defaults apply to list and show;
  explicit `--computed` values replace them and `--computed none` suppresses them.
- Computed list text uses `S:<key>` and `P:<key>` columns rather than a single
  truncated computed-data preview.
- `S:<key>` and `P:<key>` sorts order the full matching set before `--limit`;
  combining either with `--cursor` returns an actionable error.
- Display aliases use the first selector that exists and can be selected like
  ordinary data columns.
- `S:<key>` and `P:<key>` selectors work in semantic pipe filtering,
  projection, sorting, grouping, aggregation, and value extraction when selected
  explicitly or by a per-class default.
- Computed path completion prefers the class schema and only samples object data
  when the class has no schema.

## Pipe DSL And Redirects

Run semantic pipeline checks against real object output:

```text
object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4
object list --class Hosts | F os_version 26
object list --class Hosts | F data.cpu.cores>=8
object list --class Hosts | V 129.240
object list --class Hosts | K ipv4
object list --class Hosts | G os_version AS "OS Version" | A count AS Hosts | S Hosts desc AS num | L 10
object list --class Hosts | VALUE Name | C
object list --json --class Hosts | JQ 'map({Name, os_version})' | L 5
```

Run redirect checks in a temporary directory:

```text
config show --output json > /tmp/hubuum-config.json
object list --class Hosts | P Name os_version > /tmp/hubuum-hosts.txt
object list --class Hosts | VALUE Name > each:/tmp/hubuum-host-{value}.txt
```

From a POSIX shell, verify direct application-level redirects and color
handling (the operators are escaped so the shell does not consume them):

```sh
hubuum-cli --color auto help \> /tmp/hubuum-help.txt
hubuum-cli config show \> each:/tmp/hubuum-config-{n}.txt
```

Expected results:

- Pipes operate on structured output, not rendered table glyphs.
- Broad `F` searches can match hidden semantic values, while `F field regex`
  limits the search to one selector.
- `F data.cpu.cores>=8` is evaluated as a comparison and does not create a file
  named `8`.
- The documented JQ map expression returns only `Name` and `os_version`.
- Grouped aggregate output suppresses cursor pagination prompts after terminal grouping stages.
- `>` truncates, `>>` appends, and `each:<template>` creates one file per semantic row or value.
- Field placeholders in `each:<template>` are sanitized before writing.
- `/tmp/hubuum-help.txt` contains no ANSI escape sequences under `--color auto`.

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
collection principal-permissions cli-smoke --principal-kind group --principal admins
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
event sink list
event subscription list
event delivery health
event delivery list --limit 5
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
help pipe jq
help shell completion
```

Expected results:

- Help text colors only command fragments when color is enabled.
- Dense table bands are subtle on dark backgrounds.
- Theme selection works at runtime and persists through config when requested.

## Administrative Configuration, Backups, And Restore

With an administrator account, inspect the redacted server v0.0.2 configuration and
exercise backup handling:

```text
admin config
admin config --output json
backup submit
backup show <task-id>
backup download <task-id> --file /tmp/hubuum-smoke-backup.json
backup create --file /tmp/hubuum-smoke-backup-direct.json
```

Only on a disposable server, test the destructive two-step restore flow:

```text
restore stage --file /tmp/hubuum-smoke-backup.json --receipt /tmp/hubuum-smoke-restore.json
restore status --receipt /tmp/hubuum-smoke-restore.json
restore confirm --receipt /tmp/hubuum-smoke-restore.json --yes
```

Expected results:

- Configuration secrets remain redacted.
- Backup and receipt files have mode `0600` on Unix and are not overwritten without
  `--force`.
- Staging validates without replacing data; confirmation replaces all data and
  invalidates the current bearer token.

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
