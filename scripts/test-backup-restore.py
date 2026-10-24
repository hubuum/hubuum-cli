#!/usr/bin/env python3
"""Exercise the CLI against a pinned, disposable server; requires Python 3.11+."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

from history_checks import verify_history


ROOT = Path(__file__).resolve().parent.parent


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


def eventually(action, seconds=120):
    deadline = time.monotonic() + seconds
    while True:
        try:
            return action()
        except (OSError, subprocess.CalledProcessError, ValueError, AssertionError):
            if time.monotonic() >= deadline:
                raise
            time.sleep(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/hubuum-cli")
    parser.add_argument("--runtime", default=shutil.which("podman") or shutil.which("docker"))
    args = parser.parse_args()
    assert args.runtime, "Docker or Podman is required"
    binary = str(args.binary.resolve())
    assert Path(binary).is_file(), "Run cargo build --locked first"
    manifest = (ROOT / "Cargo.toml").read_text()
    image = re.search(r'^server-image = "([^"]+)"', manifest, re.M)[1]
    version = re.search(r'^server-version = "([^"]+)"', manifest, re.M)[1]
    postgres = (ROOT / "tests/fixtures/postgres/Dockerfile").read_text().split()[1]
    assert "@sha256:" in image and "@sha256:" in postgres
    prefix = "hubuum-cli-restore-" + uuid.uuid4().hex[:12]
    network, db, server, executor = [prefix + suffix for suffix in ["-net", "-db", "-server", "-executor"]]

    def container(*command):
        return run(args.runtime, *command)

    print(f"Testing server {version}: {image}", flush=True)
    try:
        print("Pulling pinned PostgreSQL image", flush=True)
        container("pull", postgres)
        print("Pulling pinned server image", flush=True)
        container("pull", image)
        print("Starting disposable database and applying migrations", flush=True)
        container("network", "create", network)
        container("run", "-d", "--name", db, "--network", network,
                  "-e", "POSTGRES_USER=hubuum", "-e", "POSTGRES_PASSWORD=disposable-test",
                  "-e", "POSTGRES_DB=hubuum", postgres)
        # The image's temporary initialization server accepts Unix-socket
        # connections before the final server starts listening on TCP.
        eventually(lambda: container("exec", db, "pg_isready", "-h", "127.0.0.1",
                                     "-U", "hubuum", "-d", "hubuum"))
        db_env = f"HUBUUM_DATABASE_URL=postgres://hubuum:disposable-test@{db}/hubuum"
        container("run", "--rm", "--network", network, "-e", db_env,
                  "--entrypoint", "hubuum-admin", image, "--migrate")
        container("run", "-d", "--name", server, "--network", network,
                  "-p", "127.0.0.1::8080", "-e", db_env,
                  "-e", "HUBUUM_BIND_IP=0.0.0.0", "-e", "HUBUUM_BIND_PORT=8080",
                  "-e", "HUBUUM_CLIENT_ALLOWLIST=*", image)
        def published_port():
            mapping = container("port", server, "8080/tcp").strip()
            if not mapping:
                raise ValueError("Disposable server has no published port yet")
            return mapping.rsplit(":", 1)[1]

        port = eventually(published_port)
        base = f"http://127.0.0.1:{port}"

        def api(method, path, data=None, token=None):
            headers = {"Content-Type": "application/json"}
            if token:
                headers["Authorization"] = "Bearer " + token
            request = urllib.request.Request(base + path, method=method, headers=headers,
                                             data=None if data is None else json.dumps(data).encode())
            with urllib.request.urlopen(request, timeout=30) as response:
                body = response.read()
                return json.loads(body) if body else None

        def reset_and_login():
            output = container("exec", server, "hubuum-admin", "--reset-password", "admin")
            match = re.search(r'"password"\s*:\s*"([^"]+)"|reset to:\s*(\S+)', output)
            assert match, "Administrator password reset did not return a password"
            password = next(value for value in match.groups() if value)
            return api("POST", "/api/v1/auth/login", {"name": "admin", "password": password, "client_id": "cli-backup-test"})["access_token"], password

        # The stack is constructed here and never accepts an external server URL.
        token, password = eventually(reset_and_login)
        api_version = api("GET", "/api-doc/openapi.json")["info"]["version"]
        assert api_version == version, (api_version, version)
        delegated_env = dict(os.environ, HUBUUM_E2E_BASE_URL=base,
                             HUBUUM_E2E_ADMIN_PASSWORD=password,
                             HUBUUM_E2E_CLI_BINARY=binary)
        print(run(sys.executable, str(ROOT / "scripts/test-collection-integrations.py"),
                  env=delegated_env), end="", flush=True)
        print("Starting restore executor and CLI checks", flush=True)
        container("run", "-d", "--name", executor, "--network", network, "-e", db_env,
                  "--entrypoint", "hubuum-admin", image, "--restore-executor")

        with tempfile.TemporaryDirectory(prefix=prefix) as temporary:
            directory = Path(temporary)
            token_file = directory / "token"
            config = directory / "config.toml"
            config.write_text("[completion]\ndisable_api_related = true\n")
            token_file.write_text(token)
            token_file.chmod(0o600)
            approval_file = directory / "approval-password"
            approval_file.write_text(password)
            approval_file.chmod(0o600)
            env = {key: value for key, value in os.environ.items() if not key.startswith("HUBUUM_CLI__")}
            env.update(XDG_CONFIG_HOME=temporary, XDG_DATA_HOME=temporary, XDG_STATE_HOME=temporary)

            # XDG directories isolate the legacy cache on Linux. Other platforms
            # must not consult a developer's existing password-login cache here.
            if sys.platform == "linux":
                # Exercise the CLI's password-login path, not only injected bearers.
                session_result = subprocess.run(
                    [binary, "--config", str(config), "--hostname", "127.0.0.1", "--port", port,
                     "--protocol", "http", "--username", "admin", "admin", "config", "--output", "json"],
                    text=True, capture_output=True, timeout=60,
                    env=dict(env, HUBUUM_CLI__SERVER__PASSWORD=password),
                )
                assert session_result.returncode == 0, "CLI session login or authenticated config read failed"
                assert json.loads(session_result.stdout), "CLI session returned no administrator configuration"
                assert password not in session_result.stdout + session_result.stderr
                assert json.loads((directory / "hubuum_cli/token.json").read_text()) == []
                cache = directory / "hubuum_cli/sessions.json"
                saved = json.loads(cache.read_text())
                assert len(saved) == 1 and password not in cache.read_text()
                session_args = [binary, "--config", str(config), "--hostname", "127.0.0.1", "--port", port,
                                "--protocol", "http", "--username", "admin", "admin", "config", "--output", "json"]
                restarted = subprocess.run(session_args, text=True, capture_output=True, timeout=60, env=env)
                assert restarted.returncode == 0, "CLI restart did not reuse the saved session"
                original_refresh = saved[0]["session"]["pair"]["refresh_token"]
                saved[0]["session"]["refresh_at"] = "2020-01-01T00:00:00Z"
                saved[0]["session"]["pair"]["access_expires_at"] = "2020-01-01T00:00:00Z"
                cache.write_text(json.dumps(saved))
                processes = [subprocess.Popen(session_args, text=True, stdout=subprocess.PIPE,
                                              stderr=subprocess.PIPE, env=env) for _ in range(2)]
                for process in processes:
                    stdout, stderr = process.communicate(timeout=60)
                    assert process.returncode == 0, "Concurrent CLI failed to resume/refresh the saved session"
                    assert json.loads(stdout) and password not in stdout + stderr
                assert json.loads(cache.read_text())[0]["session"]["pair"]["refresh_token"] != original_refresh
                print("PASS: CLI sessions survive restarts and coordinate refresh across processes", flush=True)

                # Concurrent cache misses must join one new session rather than
                # replace the session another process has just begun using.
                cold_directory = directory / "cold-start"
                cold_env = dict(env, XDG_DATA_HOME=str(cold_directory),
                                HUBUUM_CLI__SERVER__PASSWORD=password)
                for expired in [False, True]:
                    cold_cache = cold_directory / "hubuum_cli/sessions.json"
                    if expired:
                        saved = json.loads(cold_cache.read_text())
                        saved[0]["session"]["refresh_at"] = "2020-01-01T00:00:00Z"
                        for key in ["access_expires_at", "refresh_expires_at", "absolute_expires_at"]:
                            saved[0]["session"]["pair"][key] = "2020-01-01T00:00:00Z"
                        cold_cache.write_text(json.dumps(saved))
                    processes = [subprocess.Popen(session_args, text=True, stdout=subprocess.PIPE,
                                                  stderr=subprocess.PIPE, env=cold_env) for _ in range(2)]
                    for process in processes:
                        stdout, stderr = process.communicate(timeout=60)
                        assert process.returncode == 0, "Concurrent cold-start CLI login failed"
                        assert json.loads(stdout) and password not in stdout + stderr
                    assert len(json.loads(cold_cache.read_text())) == 1
                print("PASS: concurrent CLI cold starts recover from empty and expired caches", flush=True)

                before_memory_login = cache.read_bytes()
                memory_env = dict(env, HUBUUM_CLI__SERVER__SESSION_STORAGE="none",
                                  HUBUUM_CLI__SERVER__PASSWORD=password)
                for memory_directory in [directory, directory / "memory-only"]:
                    memory_login = subprocess.run(session_args, text=True, capture_output=True,
                                                  timeout=60, env=dict(memory_env, XDG_DATA_HOME=str(memory_directory)))
                    assert memory_login.returncode == 0, "Memory-only CLI login failed"
                    assert json.loads(memory_login.stdout)
                    assert password not in memory_login.stdout + memory_login.stderr
                    assert cache.read_bytes() == before_memory_login
                    if memory_directory != directory:
                        for name in ["sessions.json", "sessions.lock", "sessions.login.lock", "token.json"]:
                            assert not (memory_directory / "hubuum_cli" / name).exists()
                print("PASS: memory-only CLI logins preserve existing caches and create no session files", flush=True)

                def session_command(action):
                    result = subprocess.run(session_args[:-4] + ["auth", action, "--output", "json"],
                                            text=True, capture_output=True, timeout=60, env=env)
                    assert result.returncode == 0, (action, result.stderr)
                    assert password not in result.stdout + result.stderr
                    return json.loads(result.stdout)

                assert session_command("status")["status"] == "active"
                assert session_command("logout")["status"] == "logged_out"
                assert json.loads(cache.read_text()) == []
                assert session_command("status")["status"] == "no_saved_session"
                assert session_command("logout")["status"] == "no_saved_session"
                print("PASS: saved-session inspection and logout never create a fresh login", flush=True)

            def cli(*command, success=True, output="json", approve=True):
                approval_args = ["--approval-password-file", str(approval_file)] if approve else []
                command = list(command)
                output_index = command.index("|") if "|" in command else len(command)
                command[output_index:output_index] = ["--output", output]
                result = subprocess.run(
                    [binary, "--config", str(config), "--hostname", "127.0.0.1", "--port", port,
                     "--protocol", "http", "--token-file", str(token_file), *approval_args, *command],
                    text=True, capture_output=True, env=env, timeout=180,
                )
                assert (result.returncode == 0) == success, (command, result.stdout, result.stderr)
                return (json.loads(result.stdout) if output == "json" else result.stdout) if success else result.stdout + result.stderr

            groups = api("GET", "/api/v1/iam/me/groups", token=token)
            collection = api("POST", "/api/v1/collections", {
                "name": prefix, "description": "CLI restore fixture", "group_id": groups[0]["id"]
            }, token)
            cls = api("POST", "/api/v1/classes", {
                "name": prefix, "description": "CLI restore fixture", "collection_id": collection["id"],
                "json_schema": None, "validate_schema": False,
            }, token)
            objects_path = f'/api/v1/classes/{cls["id"]}/'
            obj = api("POST", objects_path, {
                "name": prefix, "description": "recover me", "collection_id": collection["id"],
                "hubuum_class_id": cls["id"], "data": {"nullable": None, "value": 42}
            }, token)
            object_path = f'{objects_path}{obj["id"]}'
            obj = api("PATCH", object_path, {"description": "recover an advanced revision"}, token)
            assert obj["revision"] > 1
            config_snapshot = cli("admin", "config")
            assert config_snapshot
            observed = cli("admin", "query-observations", "show")
            assert cli("admin", "query-observations", "enable")["runtime_override"] is True
            assert cli("admin", "query-observations", "disable")["runtime_override"] is False
            cli("admin", "query-observations", "enable" if observed["enabled"] else "disable")
            mine = cli("me", "show")
            for owner in [("me",), ("user", "--username", "admin")]:
                arguments = [owner[0], "token", "activity", *owner[1:], "--token-id", str(mine["token"]["id"]), "--all"]
                activity = cli(*arguments)
                assert {"session_id", "creation", "sources", "best_effort", "collection_enabled", "retention_days", "source_cap", "overflow_since"} <= activity.keys(), activity
                assert activity["best_effort"] is True
            print("PASS: shared query-observation controls and current/user credential activity", flush=True)

            # Provider presets remain ordinary webhooks. Preview through the
            # real server renderer without resolving secrets or sending chat.
            saved_events = api("GET", f'/api/v1/events?collection_id={collection["id"]}&entity_type=object&limit=1', token=token)
            event_id = saved_events[0]["event_id"]
            for target in ["slack", "mattermost", "discord"]:
                sink_name = prefix + "-" + target
                sink = cli("event", "sink", "create", "--name", sink_name,
                           "--target", target, "--url-secret-ref", "unresolved_test_url",
                           "--enabled", "false")
                assert sink["kind"] == "webhook" and "target" not in sink
                assert sink["delivery_policy"]["min_interval_ms"] == 1000
                cli("event", "sink", "grant", "--name", sink_name, "--collection", prefix)
                subscription = cli("event", "subscription", "create", "--collection", prefix,
                                   "--sink", sink_name, "--name", sink_name,
                                   "--entity-types", "object", "--actions", "created,updated",
                                   "--enabled", "false")
                preview = cli("event", "sink", "preview", "--name", sink_name,
                              "--subscription-id", str(subscription["id"]), "--event", event_id)
                assert preview["sink_kind"] == "webhook", preview
                payload = preview["payload"]
                message = payload["content" if target == "discord" else "text"]
                assert "[TEST]" in message and "Hubuum:" in message, payload
                if target == "discord":
                    assert len(message) <= 1900 and payload["allowed_mentions"] == {"parse": []}
                changed = cli("event", "sink", "update", "--sink", sink_name,
                              "--delivery-policy", '{"min_interval_ms":2000}')
                assert changed["delivery_policy"]["min_interval_ms"] == 2000
                cleared = cli("event", "sink", "update", "--sink", sink_name,
                              "--delivery-policy", '{}')
                assert not cleared.get("delivery_policy") or cleared["delivery_policy"].get("min_interval_ms") is None
            print("Webhook presets and server previews passed for Slack, Mattermost, and Discord", flush=True)

            login_events = api("GET", "/api/v1/events?entity_type=authentication&limit=100", token=token)
            login_event = next(event for event in login_events if event["metadata"].get("subject", {}).get("is_admin"))
            system_name = prefix + "-login-alerts"
            system = cli("event", "subscription", "create", "--system", "--name", system_name,
                         "--sink", sink_name, "--entity-types", "authentication", "--actions", "succeeded",
                         "--filter", '{"subject_is_admin":true}', "--enabled", "false")
            assert system["filter"]["subject_is_admin"] and "collection_id" not in system
            assert cli("event", "subscription", "show", "--system", "--name", system_name)["id"] == system["id"]
            assert any(item["id"] == system["id"] for item in cli("event", "subscription", "list", "--system"))
            updated_system = cli("event", "subscription", "update", "--system", "--subscription", system_name,
                                 "--filter", '{"subject_identity_scopes":["local"],"subject_is_admin":true}')
            assert updated_system["filter"]["subject_identity_scopes"] == ["local"]
            notification = ["--name", sink_name, "--subscription-id", str(system["id"]), "--event", login_event["event_id"]]
            assert cli("event", "sink", "preview", *notification)["sink_kind"] == "webhook"
            delivery = cli("event", "sink", "test", *notification)
            assert delivery["purpose"] == "test" and delivery["subscription_id"] == system["id"]
            assert cli("event", "delivery", "show", str(delivery["id"]))["purpose"] == "test"
            assert "exactly one" in cli("event", "subscription", "list", "--system", "--collection", prefix, success=False)
            cli("event", "subscription", "delete", "--system", "--name", system_name)
            print("PASS: system subscription CRUD, authentication filters, CLI notification preview/test and delivery status", flush=True)

            # Detail and relation reads reuse graph roots and embedded metadata.
            # Keep the root and related class in different collections so the
            # root collection still needs a lookup after related-class expansion.
            related_name = prefix + "-related"
            related_collection = api("POST", "/api/v1/collections", {
                "name": prefix + "-related-collection", "description": "Related collection",
                "group_id": groups[0]["id"],
            }, token)
            related_class = api("POST", "/api/v1/classes", {
                "name": related_name, "description": "Related read fixture",
                "collection_id": related_collection["id"], "json_schema": None, "validate_schema": False,
            }, token)
            related_object = api("POST", f'/api/v1/classes/{related_class["id"]}/', {
                "name": "neighbor", "description": "Related object",
                "collection_id": related_collection["id"], "hubuum_class_id": related_class["id"],
                "data": None,
            }, token)
            relation = cli("relation", "class", "create", "--class-a", related_name, "--class-b", prefix,
                           "--forward-label", "contains", "--reverse-label", "located in")
            assert relation["forward_label"] == "located in" and relation["reverse_label"] == "contains", relation
            cli("relation", "object", "create", "--class-a", prefix, "--object-a", prefix,
                "--class-b", related_name, "--object-b", "neighbor")
            hydrated = cli("object", "hydrate", "--class-id", str(cls["id"]), "--object-id", str(obj["id"]),
                           "--target-class-id", str(related_class["id"]), "--max-depth", "1", "--path-limit", "1")
            assert hydrated["root_object_id"] == obj["id"] and hydrated["paths"], hydrated
            assert "truncated" in hydrated and "next_cursor" not in hydrated
            historical_at = datetime.now(timezone.utc).isoformat()
            scope = ["--class-id", str(cls["id"])]
            object_scope = scope + ["--object-id", str(obj["id"])]
            timeline = cli("history", "timeline", "object", *object_scope, "--limit", "1", "--all")
            assert len(timeline["items"]) >= 2 and timeline["next_cursor"] is None, timeline
            marker = timeline["items"][0]
            snapshot = cli("history", "show", *object_scope, "--id", str(marker["history_id"]))
            assert snapshot["history_id"] == marker["history_id"] and "data" in snapshot
            assert cli("history", "timeline", "class", *scope)["items"]
            population = cli("history", "objects", *scope, "--at", historical_at, "--all")
            assert population["coverage"] == "retained_history" and population["class"]["id"] == cls["id"]
            assert any(item["id"] == obj["id"] for item in population["objects"])
            for selected in [scope, object_scope]:
                for at_args in [[], ["--at", historical_at]]:
                    relationships = cli("history", "relations", *selected, *at_args, "--all")
                    assert relationships["coverage"] == "retained_history" and relationships["relations"]
            computed = cli("history", "computed", *object_scope, "--at", historical_at)
            assert computed["mode"] == "historical_recomputation" and computed["personal"] == "unsupported"
            retired = api("POST", objects_path, {
                "name": "123", "description": "history identity fixture", "collection_id": collection["id"],
                "hubuum_class_id": cls["id"], "data": None,
            }, token)
            api("DELETE", f'{objects_path}{retired["id"]}', token=token)
            replacement = api("POST", objects_path, {
                "name": "123", "description": "replacement identity fixture", "collection_id": collection["id"],
                "hubuum_class_id": cls["id"], "data": None,
            }, token)
            named = cli("history", "object-name", *scope, "--name", "123", "--limit", "1", "--all")
            assert {retired["id"], replacement["id"]} <= {item["id"] for item in named["items"]}
            deleted_marker = next(item for item in named["items"] if item["id"] == retired["id"])
            deleted = cli("history", "show", *scope, "--object-id", str(retired["id"]), "--id", str(deleted_marker["history_id"]))
            assert deleted["id"] == retired["id"] and deleted["history_id"] == deleted_marker["history_id"]
            api("DELETE", f'{objects_path}{replacement["id"]}', token=token)
            print("PASS: history timelines, direct/deleted snapshots, literal names, populations, relations, computed values, labels, and hydration", flush=True)
            verify_history(api, cli, token, collection)
            shown = cli("object", "show", "--class", prefix, "--name", prefix, "--data", "--json")
            assert shown["id"] == obj["id"] and shown["data"] == obj["data"]
            assert shown["description"] == obj["description"] and shown["collection"] == prefix
            assert shown["class"] == prefix
            assert shown["related_objects"] == [{
                "id": related_object["id"], "class": related_name, "name": "neighbor",
                "collection": related_collection["name"], "depth": 1, "children": [],
            }], shown
            text = cli("object", "show", "--class", prefix, "--name", prefix, "--data", output="text")
            assert related_name + "/neighbor" in text and "nullable" in text and "42" in text
            assert text.index("Relations") < text.index("nullable")
            projected = cli("object", "show", "--class", prefix, "--name", prefix,
                            "|", "P", "name", "data", "related_objects")
            assert projected == {key: shown[key] for key in ["name", "data", "related_objects"]}
            listed = cli("object", "list", "--class", prefix)
            assert listed[0]["data"] == obj["data"] and listed[0]["collection"] == prefix
            class_details = cli("class", "show", "--name", prefix)
            assert class_details["class"]["collection"]["name"] == prefix
            assert cli("relation", "class", "graph", "--root-class", prefix)
            assert cli("relation", "class", "list", "--root-class", prefix)

            related = cli("relation", "object", "list", "--root-class", prefix, "--root-object", prefix)
            assert related[0]["collection"] == related_collection["name"] and related[0]["path"] == ["neighbor"]
            graph = cli("relation", "object", "graph", "--root-class", prefix, "--root-object", prefix)
            neighbor = next(item for item in graph["objects"] if item["id"] == related_object["id"])
            assert neighbor["collection"] == related_collection["name"] and neighbor["path"] == ["neighbor"]
            entity_events = cli("audit", "resource", "--resource", "object", "--class", prefix, "--name", prefix, "--all")
            relation_events = cli("audit", "resource", "--resource", "object", "--class", prefix, "--name", prefix, "--include-relations", "--all")
            entity_events = entity_events["items"]
            relation_events = relation_events["items"]
            assert len(relation_events) > len(entity_events), {
                "entity": [(event["entity_type"], event["action"]) for event in entity_events],
                "relations": [(event["entity_type"], event["action"]) for event in relation_events],
                "direct": [(event["entity_type"], event["action"]) for event in api("GET", object_path + "/events?include_relations=true", token=token)],
            }
            assert any("relation" in event["entity_type"] for event in relation_events)
            assert "requires" in cli("audit", "resource", "--resource", "collection", "--name", prefix, "--include-relations", success=False)
            print("PASS: object detail text/JSON/pipeline, object lists, class details, and relation reads", flush=True)

            # Create classes and objects in opposite orders to exercise endpoint
            # labels, with one Host connected to two Jacks and an empty Jack.
            room_classes = {}
            for kind in ["Rooms", "Jacks", "Hosts"]:
                room_classes[kind] = api("POST", "/api/v1/classes", {
                    "name": prefix + "-" + kind, "description": "Room graph fixture",
                    "collection_id": collection["id"], "json_schema": None, "validate_schema": False,
                }, token)
            room_objects = {}
            for kind, name, data in [
                ("Hosts", "host.example", {"facts": {
                    "network": {"default_ipv4": {"address": "192.0.2.10"}},
                    "identity": {"fqdn": "host.example"},
                    "hardware": {"processor": {"summary": "Fixture CPU"}},
                }}),
                ("Jacks", "jack-a", {"comment": "first jack"}),
                ("Jacks", "jack-b", None),
                ("Jacks", "jack-empty", {}),
                ("Rooms", "B701", {"comment": "fixture room"}),
            ]:
                fixture_class = room_classes[kind]
                room_objects[name] = api("POST", f'/api/v1/classes/{fixture_class["id"]}/', {
                    "name": name, "description": "Room graph fixture",
                    "collection_id": collection["id"], "hubuum_class_id": fixture_class["id"],
                    "data": data,
                }, token)
            for left, right in [("Rooms", "Jacks"), ("Jacks", "Hosts")]:
                cli("relation", "class", "create", "--class-a", room_classes[left]["name"],
                    "--class-b", room_classes[right]["name"])
            for jack in ["jack-a", "jack-b", "jack-empty"]:
                cli("relation", "object", "create", "--class-a", room_classes["Rooms"]["name"],
                    "--object-a", "B701", "--class-b", room_classes["Jacks"]["name"], "--object-b", jack)
                if jack != "jack-empty":
                    cli("relation", "object", "create", "--class-a", room_classes["Jacks"]["name"],
                        "--object-a", jack, "--class-b", room_classes["Hosts"]["name"],
                        "--object-b", "host.example")
            # Related predicates share the ordinary object-list paging and output path.
            hosts = ["object", "list", "--class", room_classes["Hosts"]["name"]]
            room_group = ["--related", "room=" + room_classes["Rooms"]["name"],
                          "--related-where", "room.name", "equals", "B701"]
            assert cli(*hosts, *room_group, "|", "C") == [0]  # Direct is the default.
            room_group += ["--related-depth", "room=2"]
            assert cli(*hosts, *room_group, "|", "C") == [1]
            assert cli(*hosts, *room_group, "--computed", "all", "|", "C") == [1]
            assert cli(*hosts, *room_group, "--computed", "all", "--sort", "S:rank", "desc",
                       "--all", "|", "C") == [1]
            assert cli(*hosts, "--related-id", f'jack={room_classes["Jacks"]["id"]}',
                       "--related-where", "jack.name", "equals", "jack-a",
                       "--related-where", "jack.data.comment", "equals", "first jack", "|", "C") == [1]
            # Two conditions in one alias must match one target, not two different Jacks.
            assert cli(*hosts, "--related", "jack=" + room_classes["Jacks"]["name"],
                       "--related-where", "jack.name", "equals", "jack-b",
                       "--related-where", "jack.data.comment", "equals", "first jack", "|", "C") == [0]
            assert cli(*hosts, "--related", "a=" + room_classes["Jacks"]["name"],
                       "--related", "b=" + room_classes["Jacks"]["name"],
                       "--related-where", "a.name", "equals", "jack-a",
                       "--related-where", "b.name", "equals", "jack-b", "|", "C") == [1]
            jacks = ["object", "list", "--class", room_classes["Jacks"]["name"],
                     *room_group, "--sort", "name", "asc", "--limit", "1", "--include-total"]
            page = cli(*jacks)
            assert page["total_count"] == 3 and len(page["items"]) == 1 and page["next_cursor"], page
            next_page = cli(*jacks, "--cursor", page["next_cursor"])
            assert next_page["items"][0]["name"] != page["items"][0]["name"]
            assert cli(*jacks, "--all", "|", "C") == [3]
            assert cli("object", "list", "--class", room_classes["Rooms"]["name"],
                       *room_group, "|", "C") == [0]  # Traversal excludes the root itself.
            print("PASS: related object filters, depth, same-target groups, JSON predicates, computed output, pagination, and pipelines", flush=True)

            graph_args = ["relation", "object", "graph", "--root-class", room_classes["Rooms"]["name"],
                          "--root-object", "B701", "--max-depth", "2"]
            plain_graph = cli(*graph_args)
            data_graph = cli(*graph_args, "--data")
            assert all("data" not in item for item in plain_graph["objects"])
            graph_names = {item["name"] for item in data_graph["objects"]}
            assert set(room_objects) - {"B701"} <= graph_names <= set(room_objects)
            for item in data_graph["objects"]:
                assert item["data"] == room_objects[item["name"]]["data"], item
            calls = directory / "room-cli-calls"
            shim = directory / "room-cli"
            shim.write_text("#!/bin/sh\n" + "printf 'call\\n' >> " + shlex.quote(str(calls)) + "\nexec " +
                            shlex.join([binary, "--config", str(config), "--hostname", "127.0.0.1",
                                        "--port", port, "--protocol", "http", "--token-file", str(token_file)]) +
                            ' "$@"\n')
            shim.chmod(0o700)
            wrapper_env = dict(env, HUBUUM_CLI_BIN=str(shim),
                               **{f"HUBUUM_{kind.upper()}_CLASS": value["name"]
                                  for kind, value in room_classes.items()})
            wrapper = str(ROOT / "examples/hubuum-wrappers/hubuum-room")
            for identifier in ["B701", "701", str(room_objects["B701"]["id"])]:
                calls.write_text("")
                placement = json.loads(run(wrapper, identifier, "--json", env=wrapper_env))
                assert len(calls.read_text().splitlines()) == 2
                assert placement["room"]["data"] == {"comment": "fixture room"}
                assert [item["jack"]["name"] for item in placement["jacks"]] == ["jack-a", "jack-b", "jack-empty"]
                for item in placement["jacks"]:
                    assert item["jack"]["data"] == room_objects[item["jack"]["name"]]["data"]
                    expected = [] if item["jack"]["name"] == "jack-empty" else ["host.example"]
                    assert [host["name"] for host in item["hosts"]] == expected
                    for host in item["hosts"]:
                        assert host["data"] == room_objects[host["name"]]["data"]
            room_text = run(wrapper, "B701", env=wrapper_env)
            assert "B701 (fixture room)" in room_text and "jack-empty" in room_text
            assert room_text.count("192.0.2.10") == 2 and room_text.count("Fixture CPU") == 2
            print("PASS: room graph data, reversed endpoint ordering, multihomed Hosts, empty Jacks, and two CLI calls", flush=True)

            # Graph limits are server-side guards, not list pagination or truncation.
            # The root is excluded from the related-entry count, so two neighbors
            # are needed to exercise a limit of one for both graph types.
            second_name = prefix + "-second"
            second_class = api("POST", "/api/v1/classes", {
                "name": second_name, "description": "Graph limit fixture",
                "collection_id": related_collection["id"], "json_schema": None, "validate_schema": False,
            }, token)
            api("POST", f'/api/v1/classes/{second_class["id"]}/', {
                "name": "second-neighbor", "description": "Graph limit fixture",
                "collection_id": related_collection["id"], "hubuum_class_id": second_class["id"], "data": None,
            }, token)
            cli("relation", "class", "create", "--class-a", prefix, "--class-b", second_name)
            cli("relation", "object", "create", "--class-a", prefix, "--object-a", prefix,
                "--class-b", second_name, "--object-b", "second-neighbor")
            for command in [
                ("class", "show", "--name", prefix),
                ("object", "show", "--class", prefix, "--name", prefix),
                ("relation", "class", "graph", "--root-class", prefix),
                ("relation", "object", "graph", "--root-class", prefix, "--root-object", prefix),
            ]:
                assert cli(*command, "--limit", "100") == cli(*command)
                invalid = cli(*command, "--limit", "0", success=False)
                assert "greater than zero" in invalid, invalid
                exceeded = cli(*command, "--limit", "1", success=False)
                assert "limit" in exceeded.lower(), exceeded
            print("PASS: all four graph limits preserve results and reject zero/oversized graphs", flush=True)
            endpoints = ["--class-a", prefix, "--object-a", prefix,
                         "--class-b", related_name, "--object-b", "neighbor"]
            cli("relation", "object", "delete", *endpoints)
            for class_name, object_name in [(prefix, prefix), (related_name, "neighbor")]:
                events = cli("audit", "resource", "--resource", "object", "--class", class_name,
                             "--name", object_name, "--include-relations", "--all")
                assert any("relation" in event["entity_type"] and event["action"] == "deleted" for event in events["items"])
            class_events = cli("audit", "resource", "--resource", "class", "--name", prefix,
                               "--include-relations", "--all")
            assert any("relation" in event["entity_type"] for event in class_events["items"])
            cli("relation", "object", "create", *endpoints)
            print("PASS: opt-in class/object audit includes relation activity and deleted relationships from either endpoint", flush=True)



            # Search files and the terminal predicate DSL must agree on the pinned API.
            query_file = directory / "search.json"
            query_file.write_text(json.dumps({
                "version": 1, "target": {"kind": "object", "class": {"name": prefix}},
                "filter": {"op": "field", "predicate": {"field": "json_data", "path": "value", "operator": "gte", "value": 40}},
                "include_total": True, "limit": 1,
            }))
            found = cli("search", "--query-file", str(query_file), "--all")
            assert found["total"] == 1 and found["results"][0]["resource"]["id"] == obj["id"]
            native = cli("search", "--target", "object", "--class", prefix,
                         "--where", 'data.value >= 40 AND NOT name == "absent"')
            assert native["results"] == found["results"]
            streamed = cli("search", prefix, "--kind", "collection", "--stream", output="jsonl")
            events = [json.loads(line) for line in streamed.splitlines()]
            assert events[0]["event"] == "started" and events[-1]["event"] == "done"
            assert any(event["event"] == "batch" for event in events)
            for output in ["json", "jsonl"]:
                streamed = cli("search", "--query-file", str(query_file), "--stream", output=output)
                events = streamed if output == "json" else [json.loads(line) for line in streamed.splitlines()]
                assert events[0]["event"] == "started" and events[-1]["event"] == "done"
                assert events[-1]["data"]["total"] == 1
                assert [event["data"] for event in events if event["event"] == "result"] == found["results"]
            projected = cli("search", "--target", "object", "--class", prefix,
                            "--where", "data.value >= 40", "--stream", "|", "P", "name")
            assert projected == [{"name": prefix}], projected
            empty = cli("search", "--target", "object", "--class", prefix,
                        "--where", 'name == "absent"', "--stream", "|", "P", "name")
            assert empty == [], empty
            conflict = cli("search", "--target", "object", "--stream", "--all", success=False)
            assert "--all" in conflict and "--stream" in conflict
            print("PASS: typed structured query files, terminal predicates, streaming JSON/JSONL and resource pipelines", flush=True)

            # The server must reject bearer-only credential changes; the CLI then
            # requests exactly bound approval from the acting human's password.
            denied = cli("user", "create", "--username", prefix + "-unapproved", success=False, approve=False)
            assert "fresh password approval" in denied
            created = cli("user", "create", "--username", prefix + "-human")
            assert created["user"]["name"] == prefix + "-human"
            issued = cli("user", "token", "create", "--username", "admin", "--name", prefix + "-approved")
            assert issued
            new_password = directory / "new-user-password"
            new_password.write_text("integration-replacement-password")
            new_password.chmod(0o600)
            cli("user", "set-password", "--username", prefix + "-human",
                "--password-file", str(new_password), output="text")
            principal_tokens = cli("user", "token", "list", "--username", "admin")
            source = next(item for item in principal_tokens if item["name"] == prefix + "-approved")
            assert cli("user", "token", "renew", "--username", "admin", "--token-id", str(source["id"]))
            account = prefix + "-service"
            cli("service-account", "create", "--name", account, "--owner-group", groups[0]["groupname"])
            assert cli("service-account", "token", "create", "--name", account, "--token-name", prefix)
            service_tokens = cli("service-account", "token", "list", "--name", account)
            service_activity = cli("service-account", "token", "activity", "--name", account,
                                   "--token-id", str(service_tokens[0]["id"]), "--all")
            assert service_activity["creation"] is not None and service_activity["best_effort"]
            credential_import = directory / "credential-import.json"
            credential_import.write_text(json.dumps({
                "version": 2, "dry_run": True, "graph": {"principals": [{
                    "kind": "human", "name": prefix + "-imported", "provider_managed": False,
                    "identity_scope_key": {"name": "local"}, "password": "integration-import-password",
                }]},
            }))
            denied_import = cli("import", "submit", "--file", str(credential_import), success=False, approve=False)
            assert "fresh password approval" in denied_import
            imported = cli("import", "submit", "--file", str(credential_import), "--wait")
            results = imported["ImportResults"]
            assert len(results) == 1, imported
            assert cli("task", "show", str(results[0]["task_id"]))["status"] == "succeeded", imported
            print("PASS: bearer-only rejection; approved user/password/token/service-account operations and credential import dry run", flush=True)

            def schema(command, *options, success=True):
                return cli("class", "schema", command, "--class", prefix, *options, success=success)

            def completed_work(task):
                work = schema("work", "--task", str(task))
                assert work["status"] == "complete", work
                return work

            active = schema("show")["active"]["revision"]
            proposal = schema("stage", "--schema", '{"type":"object","required":["missing"]}',
                              "--validate", "true")["revision"]
            assert schema("revision", "--revision", str(proposal))["status"] == "staged"
            impact = schema("impact", "--revision", str(proposal))["task_id"]
            work = eventually(lambda: completed_work(impact))
            assert work["readiness"] == "incompatible"
            summary = cli("class", "schema", "work", "--class", prefix, "--task", str(impact), output="text")
            fields = {label.strip(): value.strip() for line in summary.splitlines()
                      if ":" in line for label, value in [line.split(":", 1)]}
            assert fields["Readiness"] == "incompatible" and fields["Newly invalid"] == "1"
            assert '"snapshot"' not in summary and "--output json" in summary
            assert len(summary) < 2000
            policy_summary = cli("class", "schema", "revision", "--class", prefix,
                                 "--revision", str(proposal), output="text")
            assert any(line.startswith("Schema ") and line.endswith(": present")
                       for line in policy_summary.splitlines())
            assert '"required"' not in policy_summary

            html = schema("generate-report", "--task", str(impact),
                          "--object-url-template", "https://inventory.example/objects/{object_id}")
            assert "<html" in html.lower()
            assert schema("report", "--task", str(impact)) == html
            schema("activate", "--revision", str(proposal), "--expected-active-revision", str(active),
                   "--impact-task", str(impact), success=False)
            assert schema("show")["active"]["revision"] == active
            assert schema("abandon", "--revision", str(proposal))["status"] == "abandoned"

            proposal = schema("stage", "--schema", '{"type":"object"}',
                              "--validate", "true")["revision"]
            impact = schema("impact", "--revision", str(proposal))["task_id"]
            assert eventually(lambda: completed_work(impact))["readiness"] == "compatible"
            activated = schema("activate", "--revision", str(proposal),
                               "--expected-active-revision", str(active), "--impact-task", str(impact))
            assert activated["active"]["revision"] == proposal
            toggle = schema("stage", "--validate", "false")
            assert toggle["json_schema"] == {"type": "object"}
            assert toggle["validate_schema"] is False
            toggle_impact = schema("impact", "--revision", str(toggle["revision"]))["task_id"]
            assert eventually(lambda: completed_work(toggle_impact))["readiness"] == "compatible"
            assert schema("show")["active"]["validate_schema"] is True
            schema("abandon", "--revision", str(toggle["revision"]))
            copied = schema("stage", "--from-revision", str(active))
            assert copied["json_schema"] is None and copied["validate_schema"] is False
            copied_impact = schema("impact", "--revision", str(copied["revision"]))["task_id"]
            assert eventually(lambda: completed_work(copied_impact))["readiness"] == "compatible"
            assert schema("show")["active"]["revision"] == proposal
            schema("abandon", "--revision", str(copied["revision"]))
            validation = schema("revalidate", "--revision", str(proposal))["task_id"]
            eventually(lambda: completed_work(validation))
            page = schema("objects", "--status", "valid", "--limit", "1")
            assert page["items"][0]["object_id"] == obj["id"]
            assert schema("revisions", "--after", str(active), "--limit", "1")[0]["revision"] > active
            tasks = cli("task", "list", "--kind", "schema_validation")
            assert tasks
            cancelled = cli("task", "cancel", str(validation), "--reason", "Lifecycle check")
            assert cancelled["kind"] == "schema_validation" and cancelled["status"] == "succeeded"
            assert schema("cancel", "--task", str(validation))["status"] == "complete"
            assert "unattempted_items" in cancelled
            # Retain the current object state as the restore expectation.
            obj = api("GET", object_path, token=token)
            print("PASS: schema staging, incompatible/compatible impact, reports, activation, compliance, "
                  "revalidation, and idempotent cancellation", flush=True)

            usage_path = f'/api/v1/classes/{cls["id"]}/query-usage'
            declaration = cli("class", "query-usage", "create", prefix, "--path", "value",
                              "--type", "numeric", "--operations", "equals")
            initial_revision = declaration["revision"]
            declaration = cli("class", "query-usage", "replace", prefix, "--id", str(declaration["id"]),
                              "--expected-revision", str(initial_revision), "--path", "value",
                              "--type", "numeric", "--operations", "equals,gte")
            assert declaration["revision"] > initial_revision
            for verb in ["replace", "delete"]:
                command = ["class", "query-usage", verb, prefix, "--id", str(declaration["id"]),
                           "--expected-revision", str(initial_revision)]
                if verb == "replace":
                    command += ["--path", "value", "--type", "numeric", "--operations", "equals"]
                assert "--expected-revision" in cli(*command, success=False)
            declarations = cli("class", "query-usage", "list", prefix)
            api_declarations = api("GET", usage_path, token=token)
            comparable = json.loads(json.dumps(api_declarations))
            for item in comparable:
                for field in ["created_at", "updated_at"]:
                    item[field] = item[field].replace("Z", "+00:00")
            assert declarations == comparable, (declarations, comparable)
            assert any(item["id"] == declaration["id"] for item in declarations)
            proposed = directory / "query-usage-patterns.json"
            proposed.write_text(json.dumps([{"path":"serial","value_type":"string","operations":["equals"]}]))
            report = cli("class", "query-usage", "analyze", prefix, "--proposed", "file://" + str(proposed))
            assert report["status"] == "insufficient_evidence", report
            assert report["observations"]["settings"]["enabled"] is False
            summary = cli("class", "query-usage", "analyze", prefix, output="text")
            assert "insufficient evidence" in summary and "Observations: disabled" in summary
            assert cli("class", "query-usage", "list", prefix) == declarations
            invalid = cli("class", "query-usage", "create", prefix, "--path", "a..b", "--type", "string", "--operations", "equals", success=False)
            assert "path" in invalid.lower()
            extra = cli("class", "query-usage", "create", prefix, "--path", "serial", "--type", "string", "--operations", "equals")
            cli("class", "query-usage", "delete", prefix, "--id", str(extra["id"]), "--expected-revision", str(extra["revision"]))
            assert cli("class", "query-usage", "list", prefix) == declarations
            print("PASS: query-usage CRUD, stale-revision rejection, bounded proposed analysis, text/JSON output, and unchanged declarations after analysis", flush=True)

            def restore_cycle(label, expected_object, *, include_history=True, wait_on_confirm=True):
                nonlocal token, password
                backup = directory / f"backup-{label}.json"
                receipt = directory / f"receipt-{label}.json"
                backup_args = ["backup", "create", "--file", str(backup)]
                if not include_history:
                    backup_args += ["--include-history", "false"]
                summary = cli(*backup_args)
                assert summary["backup"]["backup_version"] == 10
                document = json.loads(backup.read_text())
                assert document["source_version"] == version
                assert document["created_at"].endswith(("Z", "+00:00"))
                assert (document["history"] is not None) == include_history
                assert backup.stat().st_mode & 0o777 == 0o600
                assert "tokens" not in document["state"]["sections"]
                assert "password_hash" not in json.dumps(document["state"]["sections"].get("principals", []))
                assert document["state"]["sections"]["query_usage_declarations"]
                api("DELETE", object_path, token=token)
                cli("class", "query-usage", "delete", prefix, "--id", str(declaration["id"]),
                    "--expected-revision", str(declaration["revision"]))
                tasks = cli("task", "list", "--kind", "backup", "--status", "succeeded",
                            "--where", "backup_include_history", "equals", str(include_history).lower(), "--all")
                assert tasks and all(task["details"]["backup"]["retained"]["include_history"] == include_history for task in tasks)
                staged = cli("restore", "stage", "--file", str(backup), "--receipt", str(receipt))
                assert staged["status"] == "validated"
                assert "restore_capability" not in staged
                assert receipt.stat().st_mode & 0o777 == 0o600
                capability = json.loads(receipt.read_text())["capability"]
                assert capability not in json.dumps(staged)
                refused = cli("restore", "confirm", "--receipt", str(receipt), success=False)
                assert "--yes" in refused
                assert cli("restore", "status", "--receipt", str(receipt))["status"] == "validated"
                confirm_args = ["restore", "confirm", "--receipt", str(receipt), "--yes"]
                if wait_on_confirm:
                    confirm_args += ["--wait", "--timeout", "120"]
                confirmed = cli(*confirm_args)
                assert confirmed["status"] == ("succeeded" if wait_on_confirm else "confirmed")
                # A separate invocation must poll successfully with invalid credentials.
                completed = cli("restore", "wait", "--receipt", str(receipt), "--timeout", "120")
                assert completed["status"] == "succeeded"
                assert capability not in json.dumps(completed)
                assert cli("restore", "status", "--receipt", str(receipt))["status"] == "succeeded"
                try:
                    api("GET", object_path, token=token)
                except urllib.error.HTTPError as error:
                    assert error.code == 401
                else:
                    raise AssertionError("Pre-restore bearer token is still usable")
                token, password = eventually(reset_and_login)
                token_file.write_text(token)
                approval_file.write_text(password)
                assert api("GET", usage_path, token=token) == api_declarations
                recovered = api("GET", object_path, token=token)
                for field in ["id", "revision", "created_at", "updated_at", "description", "data"]:
                    assert recovered[field] == expected_object[field], (field, recovered, expected_object)
                print(f"PASS: {label}, history={include_history}, confirm --wait={wait_on_confirm}, "
                      "completion, token invalidation, password reset, revision-preserving object and query-usage recovery",
                      flush=True)

            restore_cycle("with-history", obj)
            restore_cycle("without-history", obj, include_history=False, wait_on_confirm=False)

            # v0.0.14 must restore the current history snapshots as well as live
            # revisions. Check a default backup immediately, then restore a
            # second generation after further updates and a deletion.
            followup = directory / "followup-with-history.json"
            followup_receipt = directory / "followup-receipt.json"
            cli("backup", "create", "--file", str(followup))
            assert cli("restore", "stage", "--file", str(followup),
                       "--receipt", str(followup_receipt))["status"] == "validated"
            print("PASS: default backup after history-free restore stages successfully", flush=True)

            cli("object", "modify", "--name", prefix, "--class", prefix,
                "--description", "recover the next generation", "--data", "value=43")
            updated = api("GET", object_path, token=token)
            assert updated["data"] == {"nullable": None, "value": 43}
            assert updated["revision"] > obj["revision"]
            for assignment in [".".join(["nested"] * 129) + "=1", "items[1000001]=1"]:
                error = cli("object", "modify", "--name", prefix, "--class", prefix,
                            "--data", assignment, success=False)
                assert "Limit exceeded" in error
                assert api("GET", object_path, token=token) == updated
            print("PASS: object assignments preserve siblings and reject oversized paths without writes",
                  flush=True)
            deleted = api("POST", objects_path, {
                "name": prefix + "-deleted", "description": "retain deletion history",
                "collection_id": collection["id"], "hubuum_class_id": cls["id"], "data": {},
            }, token)
            deleted_path = f'{objects_path}{deleted["id"]}'
            api("DELETE", deleted_path, token=token)
            restore_cycle("second-generation-after-mutations", updated)
            try:
                api("GET", deleted_path, token=token)
            except urllib.error.HTTPError as error:
                assert error.code == 404
            else:
                raise AssertionError("Second-generation restore resurrected a previously deleted object")
            print("PASS: second-generation restore preserves the prior deletion", flush=True)
    finally:
        if sys.exc_info()[0] is not None:
            logs = subprocess.run([args.runtime, "logs", db], text=True, capture_output=True)
            sys.stderr.write(logs.stdout + logs.stderr)
        for name in [executor, server, db]:
            subprocess.run([args.runtime, "rm", "-f", "-v", name], capture_output=True)
        subprocess.run([args.runtime, "network", "rm", network], capture_output=True)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.stderr.write(error.stdout or "")
        sys.stderr.write(error.stderr or "")
        raise
