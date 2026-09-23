#!/usr/bin/env python3
"""Exercise the CLI against a pinned, disposable server; requires Python 3.9+."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid


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
        port = container("port", server, "8080/tcp").strip().rsplit(":", 1)[1]
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
            return api("POST", "/api/v0/auth/login", {"name": "admin", "password": password})["token"], password

        # The stack is constructed here and never accepts an external server URL.
        token, password = eventually(reset_and_login)
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

            def cli(*command, success=True, output="json", approve=True):
                approval_args = ["--approval-password-file", str(approval_file)] if approve else []
                result = subprocess.run(
                    [binary, "--config", str(config), "--hostname", "127.0.0.1", "--port", port,
                     "--protocol", "http", "--token-file", str(token_file), *approval_args, *command, "--output", output],
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
            assert cli("admin", "config")

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
            print("PASS: structured query files, terminal predicates, and streaming JSONL", flush=True)

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

            def restore_cycle(label, expected_object, *, include_history=True, wait_on_confirm=True):
                nonlocal token, password
                backup = directory / f"backup-{label}.json"
                receipt = directory / f"receipt-{label}.json"
                backup_args = ["backup", "create", "--file", str(backup)]
                if not include_history:
                    backup_args += ["--include-history", "false"]
                summary = cli(*backup_args)
                assert summary["backup"]["backup_version"] == 6
                document = json.loads(backup.read_text())
                assert document["source_version"] == version
                assert document["created_at"].endswith(("Z", "+00:00"))
                assert (document["history"] is not None) == include_history
                assert backup.stat().st_mode & 0o777 == 0o600
                assert "tokens" not in document["state"]["sections"]
                assert "password_hash" not in json.dumps(document["state"]["sections"].get("principals", []))
                api("DELETE", object_path, token=token)
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
                recovered = api("GET", object_path, token=token)
                for field in ["id", "revision", "created_at", "updated_at", "description", "data"]:
                    assert recovered[field] == expected_object[field], (field, recovered, expected_object)
                print(f"PASS: {label}, history={include_history}, confirm --wait={wait_on_confirm}, "
                      "completion, token invalidation, password reset, revision-preserving object recovery",
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
