#!/usr/bin/env bash

set -euo pipefail

TEST_DIR=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
WRAPPER_DIR=$(cd -P -- "$TEST_DIR/.." && pwd)
TEST_TMP=$(mktemp -d "${TMPDIR:-/tmp}/hubuum-wrapper-tests.XXXXXX")

cleanup() {
    rm -f \
        "$TEST_TMP/created.json" \
        "$TEST_TMP/edges" \
        "$TEST_TMP/fake-hubuum-cli" \
        "$TEST_TMP/object-list-queries" \
        "$TEST_TMP/output" \
        "$TEST_TMP/error" \
        "$TEST_TMP/edges.tmp"
    rmdir "$TEST_TMP" 2>/dev/null || true
}
trap cleanup EXIT

command cat >"$TEST_TMP/fake-hubuum-cli" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail

arguments=()
while [[ $# -gt 2 ]]; do
    arguments+=("$1")
    shift
done
[[ ${1:-} == --output && ${2:-} == json ]] || exit 90
set -- "${arguments[@]}"

if [[ ${1:-} == me && ${2:-} == permissions ]]; then
    case ${FAKE_ME_PERMISSIONS_CASE:-single} in
        single)
            command cat <<'JSON'
[
  {
    "collection_id": 1,
    "collection_name": "archive",
    "grants": [{"group_id": 1, "groupname": "readers", "permissions": ["ReadObject"]}]
  },
  {
    "collection_id": 2,
    "collection_name": "inventory",
    "grants": [
      {"group_id": 2, "groupname": "operators", "permissions": ["ReadObject", "CreateObject"]}
    ]
  }
]
JSON
            ;;
        none)
            printf '%s\n' \
                '[{"collection_id":1,"collection_name":"archive","grants":[]}]'
            ;;
        multiple)
            command cat <<'JSON'
[
  {
    "collection_id": 2,
    "collection_name": "inventory",
    "grants": [{"group_id": 2, "groupname": "operators", "permissions": ["CreateObject"]}]
  },
  {
    "collection_id": 3,
    "collection_name": "staging",
    "grants": [{"group_id": 2, "groupname": "operators", "permissions": ["CreateObject"]}]
  }
]
JSON
            ;;
        malformed) printf '%s\n' '{"collection_name":"inventory"}' ;;
        fail) exit 97 ;;
        *) exit 98 ;;
    esac
elif [[ ${1:-} == object && ${2:-} == list ]]; then
    shift 2
    class_name=
    name_filter=
    while [[ $# -gt 0 ]]; do
        case $1 in
            --class) class_name=$2; shift 2 ;;
            --name) name_filter=$2; shift 2 ;;
            --limit) shift 2 ;;
            --all) shift ;;
            *) exit 91 ;;
        esac
    done
    printf '%s\t%s\n' "$class_name" "$name_filter" \
        >>"$FAKE_STATE_DIR/object-list-queries"
    case $class_name in
        Hosts)
            command cat <<'JSON' | jq --arg name_filter "$name_filter" '
                if $name_filter == "" then .
                else map(select(.name | contains($name_filter)))
                end
            '
[
  {
    "id": 1,
    "name": "primary.example.org",
    "description": "Primary",
    "collection": "inventory",
    "class": "Hosts",
    "data": {
      "facts": {
        "identity": {"hostname": "primary", "fqdn": "primary.example.org"},
        "hardware": {"serial_number": "SERIAL-1"},
        "network": {
          "default_ipv4": {"address": "192.0.2.1"},
          "interfaces": [{"mac_address": "00:11:22:33:44:55"}]
        }
      }
    }
  },
  {
    "id": 2,
    "name": "other.example.org",
    "description": "Other",
    "collection": "inventory",
    "class": "Hosts",
    "data": {"facts": {"identity": {"hostname": "other", "fqdn": "other.example.org"}}}
  }
]
JSON
            ;;
        Jacks) printf '%s\n' '[{"id":10,"name":"J-1"},{"id":11,"name":"J-2"}]' ;;
        Rooms) printf '%s\n' '[{"id":20,"name":"R-1"},{"id":21,"name":"R-2"}]' ;;
        *) exit 92 ;;
    esac
elif [[ ${1:-} == relation && ${2:-} == object && ${3:-} == list ]]; then
    shift 3
    root_class=
    root_object=
    while [[ $# -gt 0 ]]; do
        case $1 in
            --root-class) root_class=$2; shift 2 ;;
            --root-object) root_object=$2; shift 2 ;;
            --max-depth|--limit) shift 2 ;;
            --all) shift ;;
            *) exit 93 ;;
        esac
    done
    jq -Rn \
        --arg root_class "$root_class" \
        --arg root_object "$root_object" \
        '
        ([inputs | split("\t") | select(length == 2) | {
            class_a: "Hosts", object_a: .[0], class_b: "Jacks", object_b: .[1]
        }]
        + [
            {class_a: "Jacks", object_a: "J-1", class_b: "Rooms", object_b: "R-1"},
            {class_a: "Jacks", object_a: "J-2", class_b: "Rooms", object_b: "R-2"}
          ])
        | map(
            if .class_a == $root_class and .object_a == $root_object
            then {class: .class_b, name: .object_b}
            elif .class_b == $root_class and .object_b == $root_object
            then {class: .class_a, name: .object_a}
            else empty
            end
          )
        | unique_by([.class, .name])
    ' <"$FAKE_STATE_DIR/edges"
elif [[ ${1:-} == relation && ${2:-} == object \
    && ( ${3:-} == create || ${3:-} == delete ) ]]; then
    operation=$3
    shift 3
    host_name=
    jack_name=
    while [[ $# -gt 0 ]]; do
        case $1 in
            --class-a|--class-b) shift 2 ;;
            --object-a) host_name=$2; shift 2 ;;
            --object-b) jack_name=$2; shift 2 ;;
            *) exit 94 ;;
        esac
    done
    if [[ $operation == create && ${FAKE_FAIL_CREATE:-} == "$host_name/$jack_name" ]]; then
        printf '%s\n' "injected create failure" >&2
        exit 95
    fi
    if [[ $operation == create ]]; then
        printf '%s\t%s\n' "$host_name" "$jack_name" >>"$FAKE_STATE_DIR/edges"
    else
        awk -F '\t' -v host="$host_name" -v jack="$jack_name" \
            '!( $1 == host && $2 == jack )' "$FAKE_STATE_DIR/edges" \
            >"$FAKE_STATE_DIR/edges.tmp"
        mv "$FAKE_STATE_DIR/edges.tmp" "$FAKE_STATE_DIR/edges"
    fi
    printf '%s\n' '{}'
elif [[ ${1:-} == object && ${2:-} == create ]]; then
    shift 2
    name=
    collection=
    data=
    while [[ $# -gt 0 ]]; do
        case $1 in
            --name) name=$2; shift 2 ;;
            --data) data=$2; shift 2 ;;
            --collection) collection=$2; shift 2 ;;
            --class|--description) shift 2 ;;
            *) exit 96 ;;
        esac
    done
    jq -n --arg name "$name" --arg collection "$collection" --argjson data "$data" \
        '{name: $name, collection: $collection, data: $data}' >"$FAKE_STATE_DIR/created.json"
    printf '%s\n' '{}'
else
    exit 99
fi
FAKE
chmod +x "$TEST_TMP/fake-hubuum-cli"

export HUBUUM_CLI_BIN=$TEST_TMP/fake-hubuum-cli
export FAKE_STATE_DIR=$TEST_TMP

assert_edges() {
    local expected=$1
    local actual
    actual=$(sort "$TEST_TMP/edges")
    [[ $actual == "$expected" ]] || {
        printf 'expected edges:\n%s\nactual edges:\n%s\n' "$expected" "$actual" >&2
        exit 1
    }
}

if HUBUUM_CLI_BIN=$TEST_TMP/missing-hubuum-cli \
    "$WRAPPER_DIR/hubuum-host-new" \
    --collection inventory \
    --no-dns \
    --no-place \
    hypnos >"$TEST_TMP/output" 2>"$TEST_TMP/error"; then
    printf '%s\n' "expected a missing hubuum-cli executable to fail" >&2
    exit 1
fi
[[ ! -s $TEST_TMP/output ]]
error_text=$(<"$TEST_TMP/error")
[[ $error_text == *'required command not found:'*missing-hubuum-cli* ]]

printf 'primary.example.org\tJ-1\n' >"$TEST_TMP/edges"
rm -f "$TEST_TMP/object-list-queries"
"$WRAPPER_DIR/hubuum-host" --json 00-11-22-33-44-55 >"$TEST_TMP/output"
jq -e '.host.name == "primary.example.org" and .placement[0].rooms == ["R-1"]' \
    "$TEST_TMP/output" >/dev/null
queries=$(<"$TEST_TMP/object-list-queries")
[[ $queries == $'Hosts\t00-11-22-33-44-55\nHosts\t' ]]

rm -f "$TEST_TMP/object-list-queries"
"$WRAPPER_DIR/hubuum-host" primary.example.org >"$TEST_TMP/output"
output_text=$(<"$TEST_TMP/output")
[[ $output_text == *'name'*'primary.example.org'* ]]
[[ $output_text == *'jack'*'J-1'* ]]
[[ $output_text == *'room'*'R-1'* ]]
queries=$(<"$TEST_TMP/object-list-queries")
[[ $queries == $'Hosts\tprimary.example.org' ]]

"$WRAPPER_DIR/hubuum-move" primary J-2 --target-type jack --dry-run \
    >"$TEST_TMP/output"
assert_edges $'primary.example.org\tJ-1'

"$WRAPPER_DIR/hubuum-move" primary J-2 --target-type jack --mode add --yes \
    >"$TEST_TMP/output"
assert_edges $'primary.example.org\tJ-2'

printf 'primary.example.org\tJ-1\nother.example.org\tJ-2\n' >"$TEST_TMP/edges"
"$WRAPPER_DIR/hubuum-move" primary other --target-type host --mode switch --yes \
    >"$TEST_TMP/output"
assert_edges $'other.example.org\tJ-1\nprimary.example.org\tJ-2'

printf 'primary.example.org\tJ-1\n' >"$TEST_TMP/edges"
if FAKE_FAIL_CREATE=primary.example.org/J-2 \
    "$WRAPPER_DIR/hubuum-move" primary J-2 --target-type jack --mode add --yes \
    >"$TEST_TMP/output" 2>"$TEST_TMP/error"; then
    printf '%s\n' "expected the injected move failure" >&2
    exit 1
fi
assert_edges $'primary.example.org\tJ-1'
error_text=$(<"$TEST_TMP/error")
[[ $error_text == *'completed changes were rolled back'* ]]

if "$WRAPPER_DIR/hubuum-host-new" \
    --collection inventory \
    --no-dns \
    --no-place \
    --mode add \
    --yes \
    invalid.example.org >"$TEST_TMP/output" 2>"$TEST_TMP/error"; then
    printf '%s\n' "expected invalid placement options to fail" >&2
    exit 1
fi
[[ ! -e $TEST_TMP/created.json ]]

FAKE_ME_PERMISSIONS_CASE=fail "$WRAPPER_DIR/hubuum-host-new" \
    --collection inventory \
    --no-dns \
    --ipv4 192.0.2.50 \
    --yes \
    new.example.org \
    NEW-SERIAL >"$TEST_TMP/output"
jq -e '
    .name == "new.example.org"
    and .collection == "inventory"
    and .data.facts.identity.hostname == "new"
    and .data.facts.hardware.serial_number == "NEW-SERIAL"
    and .data.facts.network.default_ipv4.address == "192.0.2.50"
' "$TEST_TMP/created.json" >/dev/null

rm -f "$TEST_TMP/created.json"
"$WRAPPER_DIR/hubuum-host-new" \
    --no-dns \
    --no-place \
    --yes \
    inferred.example.org >"$TEST_TMP/output"
jq -e '
    .name == "inferred.example.org"
    and .collection == "inventory"
' "$TEST_TMP/created.json" >/dev/null

output_text=$(<"$TEST_TMP/output")
[[ $output_text == *'Inferred collection: inventory'* ]]

rm -f "$TEST_TMP/created.json"
if FAKE_ME_PERMISSIONS_CASE=multiple "$WRAPPER_DIR/hubuum-host-new" \
    --no-dns \
    --no-place \
    --yes \
    ambiguous.example.org >"$TEST_TMP/output" 2>"$TEST_TMP/error"; then
    printf '%s\n' "expected ambiguous collection inference to fail" >&2
    exit 1
fi
[[ ! -e $TEST_TMP/created.json ]]
error_text=$(<"$TEST_TMP/error")
[[ $error_text == *'multiple collections: inventory, staging'* ]]
[[ $error_text == *'Specify --collection to choose one.'* ]]

if FAKE_ME_PERMISSIONS_CASE=none "$WRAPPER_DIR/hubuum-host-new" \
    --no-dns \
    --no-place \
    --yes \
    unavailable.example.org >"$TEST_TMP/output" 2>"$TEST_TMP/error"; then
    printf '%s\n' "expected missing CreateObject permission to fail" >&2
    exit 1
fi
[[ ! -e $TEST_TMP/created.json ]]
error_text=$(<"$TEST_TMP/error")
[[ $error_text == *'no collection grants CreateObject'* ]]

HUBUUM_HOST_COLLECTION=environment FAKE_ME_PERMISSIONS_CASE=fail \
    "$WRAPPER_DIR/hubuum-host-new" \
    --no-dns \
    --no-place \
    --yes \
    environment.example.org >"$TEST_TMP/output"
jq -e '
    .name == "environment.example.org"
    and .collection == "environment"
' "$TEST_TMP/created.json" >/dev/null

printf 'primary.example.org\tJ-1\n' >"$TEST_TMP/edges"
HUBUUM_EXTENSION_PROTOCOL=hubuum-cli.extension/v1 \
    "$WRAPPER_DIR/hubuum-extension" host show primary \
    >"$TEST_TMP/output"
jq -e '
    .protocol == "hubuum-cli.extension/v1"
    and .status == "ok"
    and .output.shape == "detail"
    and .output.value.host.name == "primary.example.org"
' "$TEST_TMP/output" >/dev/null

HUBUUM_EXTENSION_PROTOCOL=hubuum-cli.extension/v1 \
    "$WRAPPER_DIR/hubuum-extension" host move \
    primary J-2 --target-type jack --dry-run \
    >"$TEST_TMP/output"
jq -e '
    .status == "ok"
    and .output.value.operation == "move"
    and .output.value.status == "completed"
    and (.output.value.messages | length) > 0
' "$TEST_TMP/output" >/dev/null
assert_edges $'primary.example.org\tJ-1'

if HUBUUM_EXTENSION_PROTOCOL=hubuum-cli.extension/v1 \
    "$WRAPPER_DIR/hubuum-extension" host unknown \
    >"$TEST_TMP/output"; then
    printf '%s\n' "expected unknown extension command to fail" >&2
    exit 1
fi
jq -e '
    .status == "error"
    and .error.code == "unknown_command"
' "$TEST_TMP/output" >/dev/null

printf '%s\n' "wrapper tests passed"
