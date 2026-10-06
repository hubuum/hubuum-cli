#!/usr/bin/env python3
"""Check delegated CLI webhooks against the updated server; Python 3.11+.

Set HUBUUM_E2E_BASE_URL and HUBUUM_E2E_ADMIN_PASSWORD for a disposable server.
Only uniquely named fixtures created by this test are deleted.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import uuid


def main():
    base = os.environ["HUBUUM_E2E_BASE_URL"].rstrip("/")
    password = os.environ["HUBUUM_E2E_ADMIN_PASSWORD"]
    binary = Path(__file__).resolve().parent.parent / "target/debug/hubuum-cli"
    prefix = "cli-integrations-" + uuid.uuid4().hex[:12]

    def api(method, path, data=None, token=None, approval=None):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        if approval:
            headers["X-Hubuum-Credential-Approval"] = approval
        request = urllib.request.Request(
            base + path, method=method, headers=headers,
            data=None if data is None else json.dumps(data).encode(),
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            body = response.read()
            return json.loads(body) if body else None

    admin = api("POST", "/api/v0/auth/login", {"name": "admin", "password": password})["token"]
    user_body = {"name": prefix, "password": uuid.uuid4().hex}
    approval = api("POST", "/api/v1/iam/credential-approvals", {
        "password": password, "operation": {"kind": "create_user", "user": user_body},
    }, admin)["approval"]
    user = api("POST", "/api/v1/iam/users", user_body, admin, approval)
    group = collection = None
    try:
        group = api("POST", "/api/v1/iam/groups", {"groupname": prefix}, admin)
        api("POST", f'/api/v1/iam/groups/{group["id"]}/members/{user["id"]}', token=admin)
        collection = api("POST", "/api/v1/collections", {
            "name": prefix, "description": "CLI delegated webhook test", "group_id": group["id"],
        }, admin)
        token = api("POST", "/api/v0/auth/login", user_body)["token"]
        try:
            api("GET", "/api/v1/event-sinks", token=token)
        except urllib.error.HTTPError as error:
            assert error.code == 403
        else:
            raise AssertionError("Delegated user unexpectedly accessed global sinks")
        with tempfile.TemporaryDirectory(prefix="hubuum-cli-integrations-") as temporary:
            directory = Path(temporary)
            token_file = directory / "token"
            token_file.touch(mode=0o600)
            token_file.write_text(token)
            destination_file = directory / "destination-url"
            destination_file.touch(mode=0o600)
            destination_file.write_text("https://example.test/private-hook\n")
            config = directory / "config.toml"
            config.write_text("")
            address = urllib.parse.urlsplit(base)
            env = dict(os.environ, XDG_CONFIG_HOME=temporary, XDG_DATA_HOME=temporary,
                       XDG_STATE_HOME=temporary)
            env.pop("HUBUUM_E2E_ADMIN_PASSWORD", None)

            def cli(*command):
                result = subprocess.run([
                    str(binary), "--config", str(config), "--hostname", address.hostname,
                    "--port", str(address.port or 8080), "--protocol", address.scheme,
                    "--token-file", str(token_file), *command, "--output", "json",
                ], capture_output=True, text=True, env=env, timeout=60, check=False)
                if result.returncode != 0:
                    detail = result.stdout + result.stderr
                    for secret in (token, admin, password, approval, user_body["password"]):
                        detail = detail.replace(secret, "<redacted>")
                    raise AssertionError((command[:3], detail))
                return json.loads(result.stdout)

            sink = cli("event", "sink", "create", "--collection", prefix, "--name", prefix,
                       "--target", "slack", "--destination-url-file", str(destination_file),
                       "--enabled", "false")
            assert sink["collection_id"] == collection["id"] and sink["routing"] == "fixed"
            assert "private-hook" not in json.dumps(sink)
            assert cli("event", "sink", "list", "--collection", prefix)
            assert cli("event", "sink", "show", "--collection", prefix, "--name", prefix)["id"] == sink["id"]
            cli("event", "sink", "update", "--collection", prefix, "--sink", prefix,
                "--enabled", "false")
            subscription = cli("event", "subscription", "create", "--collection", prefix,
                               "--name", prefix, "--sink", prefix, "--entity-types", "object",
                               "--actions", "updated", "--enabled", "false")
            assert subscription["sink_id"] == sink["id"]
            cli("event", "subscription", "delete", "--collection", prefix, "--name", prefix)
            cli("event", "sink", "delete", "--collection", prefix, "--name", prefix)
        print("Delegated CLI webhook and subscription lifecycle passed")
    finally:
        if collection:
            api("DELETE", f'/api/v1/collections/{collection["id"]}', token=admin)
        if group:
            api("DELETE", f'/api/v1/iam/groups/{group["id"]}', token=admin)
        api("DELETE", f'/api/v1/iam/users/{user["id"]}', token=admin)


if __name__ == "__main__":
    main()
