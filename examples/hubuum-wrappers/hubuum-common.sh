#!/usr/bin/env bash

# Shared implementation for the hubuum-cli wrapper examples.

HUBUUM_CLI_BIN=${HUBUUM_CLI_BIN:-hubuum-cli}
HUBUUM_HOSTS_CLASS=${HUBUUM_HOSTS_CLASS:-Hosts}
HUBUUM_JACKS_CLASS=${HUBUUM_JACKS_CLASS:-Jacks}
HUBUUM_ROOMS_CLASS=${HUBUUM_ROOMS_CLASS:-Rooms}

hubuum_die() {
    printf '%s\n' "$*" >&2
    return 1
}

hubuum_require_command() {
    command -v "$1" >/dev/null 2>&1 || hubuum_die "required command not found: $1"
}

hubuum_init() {
    hubuum_require_command "$HUBUUM_CLI_BIN"
    hubuum_require_command jq
    HUBUUM_WRAPPER_TMP=$(mktemp -d "${TMPDIR:-/tmp}/hubuum-wrappers.XXXXXX") || {
        hubuum_die "unable to create a temporary directory"
        return 1
    }
    export HUBUUM_WRAPPER_TMP
    trap 'hubuum_cleanup' EXIT
    trap 'exit 130' HUP INT TERM
}

hubuum_cleanup() {
    if [[ -n ${HUBUUM_WRAPPER_TMP:-} && -d ${HUBUUM_WRAPPER_TMP:-} ]]; then
        rm -f \
            "$HUBUUM_WRAPPER_TMP/hosts.json" \
            "$HUBUUM_WRAPPER_TMP/jacks.json" \
            "$HUBUUM_WRAPPER_TMP/rooms.json" \
            "$HUBUUM_WRAPPER_TMP/object-candidates.json.tmp" \
            "$HUBUUM_WRAPPER_TMP/objects.json.tmp" \
            "$HUBUUM_WRAPPER_TMP/resolve-error"
        rmdir "$HUBUUM_WRAPPER_TMP" 2>/dev/null || true
    fi
}

hubuum_json() {
    "$HUBUUM_CLI_BIN" "$@" --output json
}

hubuum_normalize_object_list() {
    jq -e '
        if type == "array" then .
        elif type == "object" and (.items | type) == "array" then .items
        else error("unexpected object-list response")
        end
    ' "$1"
}

hubuum_infer_create_collection() {
    local class_name=$1
    local permissions
    local candidates
    local count

    if ! permissions=$(hubuum_json me permissions); then
        hubuum_die "unable to inspect CreateObject permissions; specify --collection"
        return 1
    fi
    if ! candidates=$(jq -ce '
        if type != "array" then
            error("expected an array")
        else
            [
                .[]
                | select(any(.grants[]?.permissions[]?; . == "CreateObject"))
                | if (.collection_name | type) != "string"
                       or (.collection_name | length) == 0
                  then error("writable collection has no name")
                  else {id: .collection_id, name: .collection_name}
                  end
            ]
            | unique_by([.id, .name])
        end
    ' <<<"$permissions"); then
        hubuum_die "hubuum-cli returned unexpected data for me permissions; specify --collection"
        return 1
    fi

    count=$(jq 'length' <<<"$candidates") || return
    if [[ $count -eq 1 ]]; then
        jq -r '.[0].name' <<<"$candidates"
        return
    fi
    if [[ $count -eq 0 ]]; then
        hubuum_die \
            "unable to infer a collection for $class_name: no collection grants CreateObject; specify --collection"
        return 1
    fi

    printf 'unable to infer a collection for %s: CreateObject is granted on multiple collections: ' \
        "$class_name" >&2
    jq -r 'map(.name) | sort | join(", ")' <<<"$candidates" >&2
    printf '%s\n' "Specify --collection to choose one." >&2
    return 1
}

hubuum_objects() {
    local class_name=$1
    local cache_name=$2
    local cache_file=$HUBUUM_WRAPPER_TMP/$cache_name.json
    local temporary=$HUBUUM_WRAPPER_TMP/objects.json.tmp

    if [[ ! -f $cache_file ]]; then
        if ! hubuum_json object list \
            --class "$class_name" \
            --limit 250 \
            --all >"$temporary"; then
            rm -f "$temporary"
            return 1
        fi
        if ! hubuum_normalize_object_list "$temporary" >"$cache_file"; then
            rm -f "$temporary" "$cache_file"
            hubuum_die "hubuum-cli returned an unexpected object-list response"
            return 1
        fi
        rm -f "$temporary"
    fi
    command cat "$cache_file"
}

hubuum_objects_by_name_hint() {
    local class_name=$1
    local identifier=$2
    local temporary=$HUBUUM_WRAPPER_TMP/object-candidates.json.tmp

    if ! hubuum_json object list \
        --class "$class_name" \
        --name "$identifier" \
        --limit 250 \
        --all >"$temporary"; then
        rm -f "$temporary"
        return 1
    fi
    if ! hubuum_normalize_object_list "$temporary"; then
        rm -f "$temporary"
        hubuum_die "hubuum-cli returned an unexpected object-list response"
        return 1
    fi
    rm -f "$temporary"
}

hubuum_class_objects() {
    local class_name=$1
    if [[ $class_name == "$HUBUUM_HOSTS_CLASS" ]]; then
        hubuum_objects "$class_name" hosts
    elif [[ $class_name == "$HUBUUM_JACKS_CLASS" ]]; then
        hubuum_objects "$class_name" jacks
    elif [[ $class_name == "$HUBUUM_ROOMS_CLASS" ]]; then
        hubuum_objects "$class_name" rooms
    else
        hubuum_die "unsupported wrapper class: $class_name"
    fi
}

hubuum_one_match() {
    local matches=$1
    local class_name=$2
    local identifier=$3
    local count
    count=$(jq 'length' <<<"$matches")
    if [[ $count -eq 0 ]]; then
        printf 'unable to resolve %q in class %q\n' "$identifier" "$class_name" >&2
        return 3
    fi
    if [[ $count -gt 1 ]]; then
        printf 'identifier %q matches multiple %s objects: ' "$identifier" "$class_name" >&2
        jq -r 'map(.name // "?") | sort | join(", ")' <<<"$matches" >&2
        return 4
    fi
    jq -c '.[0]' <<<"$matches"
}

hubuum_resolve_object() {
    local class_name=$1
    local identifier=$2
    local objects
    local matches

    objects=$(hubuum_class_objects "$class_name") || return
    matches=$(jq -c --arg identifier "$identifier" '
        [.[] | select(.name == $identifier)]
    ' <<<"$objects") || return
    if [[ $(jq 'length' <<<"$matches") -eq 1 ]]; then
        jq -c '.[0]' <<<"$matches"
        return
    fi
    if [[ $(jq 'length' <<<"$matches") -gt 1 ]]; then
        hubuum_one_match "$matches" "$class_name" "$identifier"
        return
    fi

    matches=$(jq -c --arg identifier "$identifier" '
        def norm: tostring | ascii_downcase | sub("\\.$"; "");
        ($identifier | norm) as $wanted
        | [.[] | select(((.name | norm) == $wanted) or ((.id | norm) == $wanted))]
    ' <<<"$objects") || return
    hubuum_one_match "$matches" "$class_name" "$identifier"
}

hubuum_match_host_objects() {
    local objects=$1
    local identifier=$2
    local matches
    local count

    matches=$(jq -c --arg identifier "$identifier" '
        [.[] | select(.name == $identifier)]
    ' <<<"$objects") || return
    count=$(jq 'length' <<<"$matches") || return
    if [[ $count -eq 1 ]]; then
        jq -c '.[0]' <<<"$matches"
        return
    fi
    if [[ $count -gt 1 ]]; then
        hubuum_one_match "$matches" "$HUBUUM_HOSTS_CLASS" "$identifier"
        return
    fi

    matches=$(jq -c --arg identifier "$identifier" '
        def norm:
            tostring
            | ascii_downcase
            | sub("\\.$"; "")
            | if test("^([0-9a-f]{2}-){5}[0-9a-f]{2}$")
              then gsub("-"; ":")
              else .
              end;
        ($identifier | norm) as $wanted
        | [
            .[]
            | . as $host
            | [
                $host.name,
                $host.id,
                $host.data.facts.identity.hostname,
                $host.data.facts.identity.fqdn,
                $host.data.facts.hardware.serial_number,
                $host.data.facts.network.default_ipv4.address,
                $host.data.facts.network.default_ipv6.address,
                $host.data.facts.network.interfaces[]?.mac_address
              ]
              | map(select(. != null) | norm) as $identifiers
            | select($identifiers | index($wanted))
            | $host
          ]
    ' <<<"$objects") || return
    count=$(jq 'length' <<<"$matches") || return
    [[ $count -gt 0 ]] || return 3
    hubuum_one_match "$matches" "$HUBUUM_HOSTS_CLASS" "$identifier"
}

hubuum_resolve_host() {
    local identifier=$1
    local objects
    local result
    local status

    if [[ ! -f $HUBUUM_WRAPPER_TMP/hosts.json ]]; then
        objects=$(hubuum_objects_by_name_hint "$HUBUUM_HOSTS_CLASS" "$identifier") || return
        if result=$(hubuum_match_host_objects "$objects" "$identifier"); then
            printf '%s\n' "$result"
            return
        else
            status=$?
        fi
        [[ $status -eq 3 ]] || return "$status"
    fi

    objects=$(hubuum_objects "$HUBUUM_HOSTS_CLASS" hosts) || return
    if result=$(hubuum_match_host_objects "$objects" "$identifier"); then
        printf '%s\n' "$result"
        return
    else
        status=$?
    fi
    if [[ $status -eq 3 ]]; then
        printf 'unable to resolve %q in class %q\n' \
            "$identifier" "$HUBUUM_HOSTS_CLASS" >&2
    fi
    return "$status"
}

hubuum_related_names() {
    local root_class=$1
    local root_name=$2
    local related_class=$3
    local objects

    objects=$(hubuum_json relation object list \
        --root-class "$root_class" \
        --root-object "$root_name" \
        --max-depth 1 \
        --limit 250 \
        --all) || return
    jq -c \
        --arg related_class "$related_class" '
        (if type == "array" then .
         elif type == "object" and (.items | type) == "array" then .items
         else error("unexpected related-object response")
         end)
        | [
            .[]
            | select(.class == $related_class)
            | .name
          ]
        | unique
        | sort_by(ascii_downcase)
    ' <<<"$objects"
}

hubuum_relation_create() {
    hubuum_json relation object create \
        --class-a "$HUBUUM_HOSTS_CLASS" \
        --object-a "$1" \
        --class-b "$HUBUUM_JACKS_CLASS" \
        --object-b "$2" >/dev/null
}

hubuum_relation_delete() {
    hubuum_json relation object delete \
        --class-a "$HUBUUM_HOSTS_CLASS" \
        --object-a "$1" \
        --class-b "$HUBUUM_JACKS_CLASS" \
        --object-b "$2" >/dev/null
}

CHANGE_OPS=()
CHANGE_HOSTS=()
CHANGE_JACKS=()

hubuum_plan_clear() {
    CHANGE_OPS=()
    CHANGE_HOSTS=()
    CHANGE_JACKS=()
}

hubuum_plan_add() {
    CHANGE_OPS+=("$1")
    CHANGE_HOSTS+=("$2")
    CHANGE_JACKS+=("$3")
}

hubuum_run_change() {
    local operation=$1
    local host_name=$2
    local jack_name=$3
    if [[ $operation == create ]]; then
        hubuum_relation_create "$host_name" "$jack_name"
    else
        hubuum_relation_delete "$host_name" "$jack_name"
    fi
}

hubuum_inverse_operation() {
    if [[ $1 == create ]]; then
        printf '%s\n' delete
    else
        printf '%s\n' create
    fi
}

hubuum_apply_plan() {
    local completed=0
    local index
    local rollback_index
    local rollback_failed=0
    local inverse

    for ((index = 0; index < ${#CHANGE_OPS[@]}; index++)); do
        if hubuum_run_change \
            "${CHANGE_OPS[$index]}" \
            "${CHANGE_HOSTS[$index]}" \
            "${CHANGE_JACKS[$index]}"; then
            completed=$((completed + 1))
        else
            printf '%s\n' "relation change failed; rolling back completed changes" >&2
            for ((rollback_index = completed - 1; rollback_index >= 0; rollback_index--)); do
                inverse=$(hubuum_inverse_operation "${CHANGE_OPS[$rollback_index]}")
                if ! hubuum_run_change \
                    "$inverse" \
                    "${CHANGE_HOSTS[$rollback_index]}" \
                    "${CHANGE_JACKS[$rollback_index]}"; then
                    rollback_failed=1
                fi
            done
            if [[ $rollback_failed -eq 1 ]]; then
                hubuum_die "move failed and rollback was incomplete"
            else
                hubuum_die "move failed; completed changes were rolled back"
            fi
            return 1
        fi
    done
}

hubuum_confirm_plan() {
    local assume_yes=$1
    local dry_run=$2
    local index
    local verb
    local answer

    if [[ ${#CHANGE_OPS[@]} -eq 0 ]]; then
        printf '%s\n' "No relation changes are needed."
        return 2
    fi
    printf '%s\n' "Planned relation changes:"
    for ((index = 0; index < ${#CHANGE_OPS[@]}; index++)); do
        if [[ ${CHANGE_OPS[$index]} == create ]]; then
            verb=add
        else
            verb=remove
        fi
        printf '  %-6s %s/%s <-> %s/%s\n' \
            "$verb" \
            "$HUBUUM_HOSTS_CLASS" \
            "${CHANGE_HOSTS[$index]}" \
            "$HUBUUM_JACKS_CLASS" \
            "${CHANGE_JACKS[$index]}"
    done
    if [[ $dry_run -eq 1 ]]; then
        printf '%s\n' "Dry run; nothing changed."
        return 2
    fi
    if [[ $assume_yes -eq 1 ]]; then
        return 0
    fi
    if [[ ! -t 0 ]]; then
        hubuum_die "refusing to modify relations without a terminal; pass --yes"
        return 1
    fi
    read -r -p "Apply these relation changes? [y/N] " answer || {
        hubuum_die "move cancelled"
        return 1
    }
    case $(printf '%s' "$answer" | tr '[:upper:]' '[:lower:]') in
        y|yes) return 0 ;;
        *) hubuum_die "move cancelled"; return 1 ;;
    esac
}

hubuum_move_host() {
    local host_name=$1
    local target_jack=$2
    local mode=$3
    local swap_host=$4
    local assume_yes=$5
    local dry_run=$6
    local current_jacks
    local swap_jacks
    local count
    local source_jack
    local jack
    local confirmation_status

    current_jacks=$(hubuum_related_names \
        "$HUBUUM_HOSTS_CLASS" "$host_name" "$HUBUUM_JACKS_CLASS") || return
    hubuum_plan_clear

    if [[ -z $target_jack ]]; then
        while IFS= read -r jack; do
            hubuum_plan_add delete "$host_name" "$jack"
        done < <(jq -r '.[]' <<<"$current_jacks")
    elif [[ $mode == add ]]; then
        while IFS= read -r jack; do
            if [[ $jack != "$target_jack" ]]; then
                hubuum_plan_add delete "$host_name" "$jack"
            fi
        done < <(jq -r '.[]' <<<"$current_jacks")
        if ! jq -e --arg jack "$target_jack" 'index($jack) != null' \
            <<<"$current_jacks" >/dev/null; then
            hubuum_plan_add create "$host_name" "$target_jack"
        fi
    elif [[ $mode == switch ]]; then
        [[ -n $swap_host ]] || {
            hubuum_die "switch mode requires a Host occupying the target Jack"
            return 1
        }
        count=$(jq 'length' <<<"$current_jacks")
        if [[ $count -ne 1 ]]; then
            hubuum_die "cannot safely switch $host_name: expected one current Jack, found $count"
            return 1
        fi
        source_jack=$(jq -r '.[0]' <<<"$current_jacks")
        swap_jacks=$(hubuum_related_names \
            "$HUBUUM_HOSTS_CLASS" "$swap_host" "$HUBUUM_JACKS_CLASS") || return
        if [[ $(jq 'length' <<<"$swap_jacks") -ne 1 \
            || $(jq -r '.[0]' <<<"$swap_jacks") != "$target_jack" ]]; then
            hubuum_die "cannot safely switch $swap_host: it must use only Jack $target_jack"
            return 1
        fi
        if [[ $source_jack == "$target_jack" ]]; then
            printf '%s\n' "No relation changes are needed."
            return 0
        fi
        hubuum_plan_add delete "$host_name" "$source_jack"
        hubuum_plan_add delete "$swap_host" "$target_jack"
        hubuum_plan_add create "$host_name" "$target_jack"
        hubuum_plan_add create "$swap_host" "$source_jack"
    else
        hubuum_die "unsupported move mode: $mode"
        return 1
    fi

    if hubuum_confirm_plan "$assume_yes" "$dry_run"; then
        hubuum_apply_plan || return
        printf '%s\n' "Move complete."
    else
        confirmation_status=$?
        [[ $confirmation_status -eq 2 ]] || return "$confirmation_status"
    fi
}
