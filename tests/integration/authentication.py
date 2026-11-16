#!/usr/bin/env python3
"""Qualify the public CLI using hash-pinned server/provider fixtures (stdlib only).

Inputs and captured command output stay in disposable private fixture state.
Reports contain only source/image hashes and sanitized check names.
"""
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import pty
import re
import selectors
import signal
import shutil
import subprocess
import sys
import time
import traceback

ROOT = Path(__file__).resolve().parents[2]
SOURCE = json.loads((ROOT / "docs/authentication-project-source.json").read_text())
LDAP = "ghcr.io/rroemhild/docker-test-openldap@sha256:3470e15c60119a1c0392cc162cdce71edfb42b55affdc69da574012f956317cd"


def definition(fixture, profile):
    if profile == "ldap":
        return {"scope": "cli-consumer", "url": "ldaps://planetexpress.com:10636",
                "bind_dn": "cn=admin,dc=planetexpress,dc=com", "bind_password": "GoodNewsEveryone",
                "user_base_dn": "ou=people,dc=planetexpress,dc=com", "user_filter": "(uid={username})",
                "subject_attribute": "entryUUID", "group_attributes": ["memberOf"], "group_rules": []}
    if profile != "oauth":
        return {
            "issuer": fixture.issuer, "client_id": "hubuum",
            "client_authentication": "client_secret_basic", "scopes": ["openid", "profile", "email"],
            "signing_algorithms": ["RS256"], "retain_refresh_tokens": True,
            "membership": {"mode": "provider_groups", "refresh_interval_seconds": 5,
                           "max_stale_seconds": 15, "claim_source": profile,
                           "claim_pointer": "/groups", "key_mode": "full_path"},
            "admission": {"mode": "provision_without_grants"},
        }
    return {
        "profile": "json_user", "authorization_endpoint": fixture.provider_origin + "/authorize",
        "token_endpoint": f"https://host.docker.internal:{fixture.provider_port}/token",
        "profile_endpoint": f"https://host.docker.internal:{fixture.provider_port}/account",
        "client_id": "fixture", "client_authentication": "client_secret_basic", "scopes": ["identity"],
        "subject_pointer": "/account/key", "subject_type": "string", "name_pointer": "/account/display",
        "profile_headers": {"x-profile-version": "2"},
        "membership": {"mode": "provider_groups", "claim_source": "profile", "claim_pointer": "/roles",
                       "key_mode": "immutable_id", "refresh_interval_seconds": 5, "max_stale_seconds": 15},
        "admission": {"mode": "provision_without_grants"},
    }


def write_private(path, value):
    path.write_text(value)
    path.chmod(0o600)
    return path


def counts(fixture):
    return {table: int(fixture.sql(f"SELECT count(*) FROM {table}")) for table in (
        "users", "principals", "group_memberships", "group_membership_sources",
        "auth_sessions", "auth_upstream_credentials")}


class Consumer:
    def __init__(self, fixture, binary, profile):
        self.fixture, self.binary, self.profile = fixture, binary, profile
        self.results = []
        self.provider = None
        self.env = {k: v for k, v in os.environ.items() if not k.upper().startswith("HUBUUM_CLI")}
        self.profile_root = fixture.root / "cli-profile"
        self.profile_root.mkdir(mode=0o700)
        self.env["HUBUUM_CLI_PROFILE_DIR"] = str(self.profile_root)
        self.approval = write_private(fixture.root / "approval", fixture.local_password)
        self.admin = self.config("admin", password=fixture.local_password)
        self.secrets = [fixture.local_password, fixture.client_secret]
        self.browser_capture = fixture.root / "browser-capture.jsonl"
        browser_bin = fixture.root / "browser-bin"
        browser_bin.mkdir(mode=0o700)
        launcher = browser_bin / ("open" if sys.platform == "darwin" else "xdg-open")
        launcher.write_text("#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\np=Path(" + repr(str(self.browser_capture)) + ")\nwith p.open('a') as f: f.write(json.dumps({'args':sys.argv[1:],'hubuum_environment':[k for k in os.environ if k.upper().startswith('HUBUUM_')]})+'\\n')\np.chmod(0o600)\n")
        launcher.chmod(0o700)
        self.env["PATH"] = str(browser_bin) + os.pathsep + self.env.get("PATH", "")
        self.operation = None


    def config(self, name, *, password=None, provider=None, username="admin"):
        port = self.fixture.proxy_port
        # These are explicit test-only inputs, not a new native configuration parser.
        lines = ["[server]", 'hostname = "localhost"', f"port = {port}", 'protocol = "https"',
                 'ssl_validation = true', "username = " + json.dumps(username), 'session_storage = "file"',
                 "ca_file = " + json.dumps(str(self.fixture.root / "mount/ca.pem"))]
        if password is not None:
            lines.append("password = " + json.dumps(password))
        if provider is not None:
            lines.append("auth_provider = " + json.dumps(provider))
        lines.extend(["[self_update]", "enabled = false", "check_on_startup = false",
                      "[completion]", "disable_api_related = true", "[extensions]",
                      "system_roots = []", "user_roots = []"])
        return write_private(self.fixture.root / (name + ".toml"), "\n".join(lines) + "\n")

    def passed(self, name):
        self.results.append(name)
        print(f"{self.profile}: {name}", flush=True)

    def assigned_provider(self, provider):
        self.provider = self.fixture.provider_id = provider
        if self.profile not in ("oauth", "ldap"):
            client = self.fixture.provider_api("/clients?clientId=hubuum")[0]
            client["redirectUris"] = [self.fixture.origin + "/api/v1/auth/callbacks/" + provider]
            self.fixture.provider_api("/clients/" + client["id"], client, method="PUT")

    def authorize(self, uri, code, decision="approve"):
        interaction = self.fixture.sql("SELECT id::text FROM auth_interactions WHERE client_id='hubuum-cli' ORDER BY created_at DESC, id DESC LIMIT 1")
        event = {"id": interaction, "next": {"verification_uri": uri, "user_code": code}}
        if self.operation == ["me", "token", "create"]:
            event["_operation"] = True
        if self.profile == "oauth":
            if decision != "approve":
                raise RuntimeError("OAuth denial driver requires the dedicated browser fixture")
            self.fixture.authorize(event)
        else:
            self.fixture.authorize(event, decision=decision)

    def run(self, config, command, *, approve=False, browser=True, terminal=False, cancel=False, failure=False, credential_output=False, reason=None, explicit_remote=True, decision="approve"):
        args = [str(self.binary), "--config", str(config)]
        if not terminal and explicit_remote:
            args.append("--no-browser")
        self.operation = command[:3]
        if approve:
            args.extend(["--approval-password-file", str(self.approval)])
        args.extend(command)
        master = slave = None
        if terminal:
            master, slave = pty.openpty()
        process = subprocess.Popen(args, stdin=slave if terminal else subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env, bufsize=0)
        if slave is not None:
            os.close(slave)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        output = {"stdout": bytearray(), "stderr": bytearray()}
        uri = None
        deadline = time.monotonic() + 180
        interrupted = False
        try:
            while selector.get_map() and time.monotonic() < deadline:
                events = selector.select(timeout=30)
                if not events:
                    if process.poll() is not None:
                        continue
                    raise RuntimeError("CLI exceeded bounded progress interval")
                for key, _ in events:
                    line = key.fileobj.readline()
                    if not line:
                        selector.unregister(key.fileobj)
                        continue
                    output[key.data].extend(line)
                    if len(output[key.data]) > 1024 * 1024:
                        raise RuntimeError("CLI exceeded output bound")
                    if key.data != "stderr":
                        continue
                    text = line.decode()
                    if text.startswith("Provider cli-consumer: "):
                        self.assigned_provider(text.strip().split(": ", 1)[1])
                    elif text.startswith("Open "):
                        uri = text.strip()[5:]
                    elif text.startswith("Verification code: "):
                        if cancel:
                            process.send_signal(signal.SIGINT)
                            interrupted = True
                        elif browser:
                            if uri is None:
                                raise RuntimeError("Verification URI missing")
                            self.authorize(uri, text.strip().split(": ", 1)[1], decision=decision)
            status = process.wait(timeout=15)
            stdout, stderr = (output[key].decode() for key in ("stdout", "stderr"))
            for secret in self.secrets:
                if secret in stdout + stderr:
                    raise RuntimeError("CLI leaked a fixture credential")
            if any(marker in stdout + stderr for marker in ("hai1.", "hcv1.", "hca1.")):
                raise RuntimeError("CLI leaked an interaction proof")
            if status != 0 and not (failure or cancel):
                hints = sorted(set(re.findall(r"\b(?:[45][0-9]{2}|capability_unsupported|invalid_credentials|invalid_request|invalid_configuration|permission_denied|provider_unavailable|interactive_login_required)\b", stderr)))
                raise RuntimeError(f"CLI operation failed (exit {status}; safe outcome hints: {','.join(hints)})")
            if failure and status == 0:
                raise RuntimeError("CLI unexpectedly accepted failure case")
            if reason is not None and reason not in stdout + stderr:
                raise RuntimeError("CLI did not report the required explicit outcome")
            if cancel and "cleanup could not be confirmed" in stderr:
                raise RuntimeError("CLI could not confirm connected cancellation cleanup")
            if cancel and not interrupted:
                raise RuntimeError("CLI cancellation was not exercised")
            # Intentional one-time token data is an explicit command result, not diagnostics.
            result = json.loads(stdout) if status == 0 else None
            if not credential_output and isinstance(result, dict) and isinstance(result.get("token"), str):
                raise RuntimeError("Unexpected credential output")
            return result
        finally:
            selector.close()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=15)
            process.stdout.close()
            process.stderr.close()
            if master is not None:
                os.close(master)

    def qualify(self):
        self.run(self.admin, ["whoami", "--output", "json"])
        self.passed("local administrator uses ordinary persisted session before runtime setup")
        state = self.run(self.admin, ["admin", "auth", "show", "--output", "json"])
        spec = definition(self.fixture, self.profile)
        if self.profile != "ldap":
            spec["credential"] = {"action": "replace", "value": self.fixture.client_secret}
        candidate = {"group": "authentication", "expected_revision": state["expected_revision"],
                     "providers": [{"name": "cli-consumer", "kind": self.profile if self.profile in ("oauth", "ldap") else "oidc",
                                    "source": "runtime", "enabled": True, "configuration": spec}]}
        if self.profile == "ldap":
            candidate["providers"][0]["verification"] = {"username": "amy", "password": self.fixture.ldap_password}
            self.secrets.extend([self.fixture.ldap_password, spec["bind_password"]])
        path = write_private(self.fixture.root / "providers.json", json.dumps(candidate))
        before = counts(self.fixture)
        tested = self.run(self.admin, ["admin", "auth", "configure", "--file", str(path), "--output", "json"])
        if before != counts(self.fixture) or tested["activated"]:
            raise RuntimeError("CLI preview changed live identities/sessions")
        self.passed("runtime candidate preview leaves six identity/session tables unchanged")
        activated = self.run(self.admin, ["admin", "auth", "configure", "--file", str(path), "--activate", "--output", "json"], approve=True)
        if not activated["activation"]["changed"]:
            raise RuntimeError("Runtime candidate did not activate")
        self.passed("write-only encrypted runtime configuration activates with exact fresh local approval")
        external = self.config("external", provider=self.provider,
                               username="amy" if self.profile == "ldap" else "admin",
                               password=self.fixture.ldap_password if self.profile == "ldap" else None)
        first = self.run(external, ["whoami", "--output", "json"])
        principal = first["principal"]["principal_id"]
        self.passed("provider CLI login installs an ordinary persisted session")
        before = self.run(external, ["auth", "status", "--output", "json"])
        expiry = datetime.fromisoformat(before["access_expires_at"].replace("Z", "+00:00"))
        now = datetime.now(timezone.utc)
        delay = max(0, (expiry - now).total_seconds()) + 0.1
        if delay > 35:
            raise RuntimeError("Fixture must use 30-second access credentials")
        time.sleep(delay)
        after_identity = self.run(external, ["whoami", "--output", "json"])
        after = self.run(external, ["auth", "status", "--output", "json"])
        if after_identity["principal"]["principal_id"] != principal or after["session_id"] != before["session_id"] or after["access_expires_at"] == before["access_expires_at"]:
            raise RuntimeError("CLI renewal changed family/identity or failed to rotate")
        self.passed("process restart and due renewal preserve principal/family and rotate access")
        self.membership(principal)
        if self.profile == "ldap":
            write_private(self.approval, self.fixture.ldap_password)
            try:
                token = self.run(external, ["me", "token", "create", "--name", "cli-qa", "--output", "json"],
                                 approve=True, credential_output=True)
            finally:
                write_private(self.approval, self.fixture.local_password)
            self.fixture.api("/api/v1/auth/logout", {}, token=token["token"])
            self.passed("LDAP password freshness approves the exact current-principal credential operation")
            self.lifecycle(candidate, path, external, principal)
            self.run(external, ["auth", "logout", "--output", "json"])
            self.run(self.admin, ["whoami", "--output", "json"])
            self.passed("local recovery remains available after LDAP runtime lifecycle")
            return self.results
        if self.profile == "oauth":
            self.run(external, ["me", "token", "create", "--name", "cli-qa", "--output", "json"], failure=True, reason="capability_unsupported")
            self.passed("OAuth fresh approval remains an explicit unsupported capability")
        else:
            token = self.run(external, ["me", "token", "create", "--name", "cli-qa", "--output", "json"], credential_output=True)
            # Revoke deliberate token output immediately using the caller's self-logout route.
            self.fixture.api("/api/v1/auth/logout", {}, token=token["token"])
            self.passed("OIDC exact-operation fresh approval targets the current immutable principal")
        self.run(external, ["auth", "logout", "--output", "json"])
        self.passed("external session logout clears the matching persisted family")
        if self.profile != "oauth":
            self.run(external, ["whoami", "--output", "json"], decision="deny", failure=True, reason="denied")
            self.passed("browser denial is terminal and does not persist a session")
        self.run(external, ["whoami", "--output", "json"], cancel=True)
        self.passed("Ctrl-C cancels the pending remote-browser interaction with exact cleanup")
        if self.run(external, ["auth", "status", "--output", "json"])["status"] != "no_saved_session":
            raise RuntimeError("Cancelled login persisted credentials")
        self.run(external, ["whoami", "--output", "json"], explicit_remote=False,
                 browser=False, failure=True, reason="interactive_login_required")
        if self.browser_capture.exists():
            raise RuntimeError("Remote/noninteractive CLI unexpectedly launched a browser")
        self.passed("noninteractive commands require explicit remote completion and never launch a browser")
        local = self.run(external, ["whoami", "--output", "json"], terminal=True)
        if local["principal"]["principal_id"] != principal:
            raise RuntimeError("Local-browser CLI changed the provider principal")
        calls = [json.loads(line) for line in self.browser_capture.read_text().splitlines()]
        if len(calls) != 1 or calls[0]["hubuum_environment"] or calls[0]["args"] != [self.fixture.origin + "/api/v1/auth/interactions/browser"]:
            raise RuntimeError("Browser hook leaked context or received unexpected arguments")
        self.passed("local terminal browser hook receives only the public URL and no Hubuum environment")
        self.run(external, ["auth", "logout", "--output", "json"])
        self.lifecycle(candidate, path, external, principal)
        recovered = self.run(self.admin, ["whoami", "--output", "json"])
        if recovered["principal"]["name"] != "admin":
            raise RuntimeError("Local recovery changed identity")
        self.passed("local recovery remains available after provider activation and cancellation")
        return self.results

    def membership(self, principal):
        self.fixture.admin = self.fixture.api("/api/v1/auth/login", {"client_id": "cli-qualification-fixture",
            "name": "admin", "password": self.fixture.local_password})["access_token"]
        group = self.fixture.api("/api/v1/iam/groups", {"groupname": "cli-manual"}, token=self.fixture.admin)
        args = ["--group-id", str(group["id"]), "--principal-id", str(principal), "--output", "json"]
        self.run(self.admin, ["group", "member", "add", *args])
        query = f"SELECT count(*) FROM group_membership_sources WHERE group_id={int(group['id'])} AND principal_id={int(principal)} AND source='manual'"
        if self.fixture.sql(query) != "1":
            raise RuntimeError("Manual grant was not stored")
        self.run(self.admin, ["group", "member", "remove", *args])
        if self.fixture.sql(query) != "0":
            raise RuntimeError("Manual grant was not removed")
        managed = self.fixture.sql("SELECT id FROM groups WHERE managed_by <> 'local' ORDER BY id LIMIT 1")
        if managed:
            self.run(self.admin, ["group", "member", "add", "--group-id", managed,
                                 "--principal-id", str(principal), "--output", "json"], failure=True)
        self.passed("immutable external principal supports manual grants and managed groups stay read-only")

    def lifecycle(self, candidate, path, external, principal):
        self.run(self.admin, ["admin", "auth", "configure", "--file", str(path), "--activate", "--output", "json"],
                 approve=True, failure=True)
        self.passed("stale desired revision fails without replay or approval downgrade")
        edited = copy.deepcopy(candidate)
        provider = edited["providers"][0]
        provider["provider_id"] = self.provider
        provider["enabled"] = False
        if self.profile == "ldap":
            provider["configuration"].pop("bind_password")
        else:
            provider["configuration"]["credential"] = {"action": "keep"}
        edited["expected_revision"] = self.run(self.admin, ["admin", "auth", "show", "--output", "json"])["expected_revision"]
        write_private(path, json.dumps(edited))
        self.run(self.admin, ["admin", "auth", "configure", "--file", str(path), "--activate", "--output", "json"], approve=True)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if not any(method["provider_id"] == self.provider for method in self.fixture.api("/api/v1/auth/methods")["methods"]):
                break
            time.sleep(0.5)
        else:
            raise RuntimeError("Disabled provider remained advertised")
        self.run(self.admin, ["whoami", "--output", "json"])
        provider["enabled"] = True
        edited["expected_revision"] = self.run(self.admin, ["admin", "auth", "show", "--output", "json"])["expected_revision"]
        write_private(path, json.dumps(edited))
        self.run(self.admin, ["admin", "auth", "configure", "--file", str(path), "--activate", "--output", "json"], approve=True)
        self.run(external, ["auth", "logout", "--output", "json"])
        if self.run(external, ["whoami", "--output", "json"])["principal"]["principal_id"] != principal:
            raise RuntimeError("Provider re-enable changed principal")
        self.passed("runtime disable/re-enable keeps encrypted credentials, immutable identity and local recovery")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-source", required=True, type=Path)
    parser.add_argument("--image", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--profile", required=True, choices=("id_token", "userinfo", "oauth", "ldap"))
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    source = args.server_source.resolve()
    commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if commit != SOURCE["accepted_server_source"]:
        raise RuntimeError("Server fixture checkout is not accepted")
    for path, expected in SOURCE["source_live_fixture_sha256"].items():
        if hashlib.sha256((source / path).read_bytes()).hexdigest() != expected:
            raise RuntimeError("Server fixture source drift")
    image = json.loads(subprocess.check_output(["docker", "image", "inspect", args.image], text=True))[0]
    qualified = next((pin for pin in SOURCE["qualified_server_images"] if image["Id"] in
                      (pin.get("docker_image_id"), pin["manifest"], pin["config"])), None)
    if qualified is None:
        raise RuntimeError("Server image is not a qualified immutable candidate")
    sys.path.insert(0, str(source / "tests/python"))
    module = importlib.import_module("integration.oauth" if args.profile in ("oauth", "ldap") else "integration.keycloak")

    class Fixture(module.Fixture):
        def certificates(self):
            mount = super().certificates()
            if args.profile == "ldap":
                directory = mount / "ldap"
                directory.mkdir()
                module.command("openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=planetexpress.com",
                               "-keyout", str(directory / "ldap.key"), "-out", str(self.root / "ldap.csr"))
                extensions = self.root / "ldap.ext"
                extensions.write_text("subjectAltName=DNS:planetexpress.com\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
                module.command("openssl", "x509", "-req", "-in", str(self.root / "ldap.csr"), "-CA", str(mount / "ca.pem"),
                               "-CAkey", str(self.root / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(extensions),
                               "-out", str(directory / "ldap.crt"))
                (directory / "ca.crt").write_bytes((mount / "ca.pem").read_bytes())
                (directory / "ldap.key").chmod(0o644)
            return mount

        def start(self):
            super().start()
            if args.profile == "ldap":
                ldap = self.run("ldap", "--network", self.network, "--network-alias", "planetexpress.com",
                                "--mount", f"type=bind,src={self.root / 'mount/ldap'},dst=/etc/ldap/ssl,readonly", LDAP)
                deadline = time.monotonic() + 90
                while True:
                    try:
                        result = module.command("docker", "exec", ldap, "ldappasswd", "-x", "-H", "ldap://127.0.0.1:10389",
                                                "-D", "cn=admin,dc=planetexpress,dc=com", "-w", "GoodNewsEveryone",
                                                "cn=Amy Wong+sn=Kroker,ou=people,dc=planetexpress,dc=com")
                        self.ldap_password = next(line.removeprefix("New password: ") for line in result.splitlines() if line.startswith("New password: "))
                        break
                    except RuntimeError:
                        if time.monotonic() >= deadline:
                            raise
                        time.sleep(1)

        def env(self, name, values):
            if name == "server":
                values["HUBUUM_SESSION_ACCESS_SECONDS"] = "30"
            return super().env(name, values)

    fixture = Fixture(args.image, args.evidence, 48479, 48480) if args.profile in ("oauth", "ldap") else Fixture(args.image, args.evidence, 48477, 48478, args.profile)
    # The accepted fixture exposes its proxy port through its origin; keep its internals opaque.
    fixture.proxy_port = 48480 if args.profile in ("oauth", "ldap") else 48478
    snapshot = fixture.root / "cli-consumer"
    shutil.copyfile(args.binary, snapshot)
    snapshot.chmod(0o700)
    report = {"profile": args.profile, "source": SOURCE, "server_image": qualified,
              "binary_sha256": hashlib.sha256(snapshot.read_bytes()).hexdigest(),
              "controller_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "cli_source_commit": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
              "cli_source_dirty": bool(subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"], text=True)),
              "checks": [], "passed": False}
    try:
        fixture.start()
        identity_path = args.evidence / "images.json"
        identity = json.loads(identity_path.read_text())
        identity.pop("source_commit", None)
        identity.pop("source_dirty", None)
        identity.update(server_source_commit=commit, cli_source_commit=report["cli_source_commit"], cli_source_dirty=report["cli_source_dirty"])
        identity_path.write_text(json.dumps(identity, indent=2) + "\n")
        consumer = Consumer(fixture, snapshot, args.profile)
        report["checks"] = consumer.results
        consumer.qualify()
        report["passed"] = True
    finally:
        fixture.cleanup()
        args.evidence.mkdir(parents=True, exist_ok=True)
        (args.evidence / "cli-qualification.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        frames = traceback.extract_tb(error.__traceback__)
        detail = f"{type(error).__name__} at {Path(frames[-1].filename).name}:{frames[-1].lineno}"
        if type(error) is RuntimeError and Path(frames[-1].filename).resolve() == Path(__file__).resolve():
            detail += ": " + str(error)
        print(f"Native CLI qualification failed ({detail}); sanitized partial report retained.", file=sys.stderr)
        raise SystemExit(1) from None
