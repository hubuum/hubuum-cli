#!/usr/bin/env python3
"""Exercise the CLI against a pinned, disposable server; requires Python 3.9+."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
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
        eventually(lambda: container("exec", db, "pg_isready", "-U", "hubuum", "-d", "hubuum"))
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
            return api("POST", "/api/v0/auth/login", {"name": "admin", "password": password})["token"]

        # The stack is constructed here and never accepts an external server URL.
        token = eventually(reset_and_login)
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
            env = {key: value for key, value in os.environ.items() if not key.startswith("HUBUUM_CLI__")}
            env.update(XDG_CONFIG_HOME=temporary, XDG_DATA_HOME=temporary, XDG_STATE_HOME=temporary)

            def cli(*command, success=True):
                result = subprocess.run(
                    [binary, "--config", str(config), "--hostname", "127.0.0.1", "--port", port,
                     "--protocol", "http", "--token-file", str(token_file), *command, "--output", "json"],
                    text=True, capture_output=True, env=env, timeout=180,
                )
                assert (result.returncode == 0) == success, (command, result.stdout, result.stderr)
                return json.loads(result.stdout) if success else result.stdout + result.stderr

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
            assert cli("admin", "config")

            # Test a history-inclusive restore before deliberately discarding history.
            for wait_on_confirm in [True, False]:
                backup = directory / f"backup-{wait_on_confirm}.json"
                receipt = directory / f"receipt-{wait_on_confirm}.json"
                summary = cli("backup", "create", "--file", str(backup),
                              "--include-history", str(wait_on_confirm).lower())
                assert summary["backup"]["backup_version"] == 5
                document = json.loads(backup.read_text())
                assert document["source_version"] == version
                assert document["created_at"].endswith(("Z", "+00:00"))
                assert (document["history"] is not None) == wait_on_confirm
                assert backup.stat().st_mode & 0o777 == 0o600
                assert "tokens" not in document["state"]["sections"]
                assert "password_hash" not in json.dumps(document["state"]["sections"].get("principals", []))
                api("DELETE", object_path, token=token)
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
                token = eventually(reset_and_login)
                token_file.write_text(token)
                recovered = api("GET", object_path, token=token)
                assert recovered["data"] == {"nullable": None, "value": 42}
                print(f"PASS: history={wait_on_confirm}, confirm --wait={wait_on_confirm}, "
                      "completion, token invalidation, password reset, object recovery", flush=True)

            # v0.0.13 retains live revisions when restoring without history, but
            # later rejects history-inclusive backups of that same database.
            # Keep this limitation visible until a newly pinned target fixes it.
            followup = directory / "followup-with-history.json"
            followup_receipt = directory / "followup-receipt.json"
            cli("backup", "create", "--file", str(followup))
            error = cli("restore", "stage", "--file", str(followup),
                        "--receipt", str(followup_receipt), success=False)
            assert "Full backup live revisions disagree with 'collection_history'" in error
            assert not followup_receipt.exists()
            print("KNOWN SERVER LIMITATION: history-inclusive backup after history-free restore "
                  "is rejected during staging (collection_history revisions)", flush=True)
            fallback = directory / "followup-without-history.json"
            cli("backup", "create", "--file", str(fallback), "--include-history", "false")
            assert cli("restore", "stage", "--file", str(fallback),
                       "--receipt", str(followup_receipt))["status"] == "validated"
            print("PASS: history-free follow-up backup stages successfully", flush=True)
    finally:
        for name in [executor, server, db]:
            subprocess.run([args.runtime, "rm", "-f", "-v", name], capture_output=True)
        subprocess.run([args.runtime, "network", "rm", network], capture_output=True)


if __name__ == "__main__":
    main()
