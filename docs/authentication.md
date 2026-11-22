# Connect and authenticate

`help`, `help --tree`, `version`, `config show`, and `config paths` run from the local
command catalog and configuration files without logging in. `version --server`,
`auth providers`, and `metrics` make unauthenticated requests. Other API-backed
commands authenticate before execution.

If an API-backed command receives `401 Unauthorized` or the SDK reports an
expired session in the interactive REPL,
Hubuum CLI reports that the session expired or the token was revoked, then
immediately renews the session. It rereads `--token-file` credentials, uses a
configured password without prompting, or prompts for the password when needed.
Read-only commands are retried once after a successful login. Commands that may
have changed server state are not replayed; the error identifies the first
failed HTTP method and path so the current state can be reviewed safely. One-shot
commands and scripts never start this interactive recovery flow.

Global configuration flags go before the command:

```sh
hubuum-cli --hostname api.example.com --username alice object list --limit 5
```

Before requesting an interactive password, Hubuum CLI checks the server's
unauthenticated health endpoint. When `server.port` has not been configured, it
tries port 443 first and then port 8080. A port supplied by a config file, the
environment, or `--port` is authoritative and is the only port tried.

Discover identity providers before login, then select one for scoped credentials:

```sh
hubuum-cli --hostname api.example.com auth providers
hubuum-cli --hostname api.example.com --identity-scope corp-directory --username alice object list
hubuum-cli config set --key server.identity_scope --value corp-directory
```

For non-interactive automation, read a service-account bearer token from an
owner-only file. The token is not placed in the process arguments or copied into
the CLI token cache:

```sh
chmod 600 /run/secrets/hubuum.token
hubuum-cli --hostname api.example.com --token-file /run/secrets/hubuum.token object list --class Hosts
```

## Credential activity

Inspect issuance provenance and recent source addresses for a credential:

```sh
hubuum-cli me token activity --token-id 42 --limit 25
hubuum-cli user token activity --username alice --token-id 42 --all --output json
hubuum-cli service-account token activity --name inventory --token-id 42 --offset 0
```

These commands require token-management authority for the owner. Rotating access
tokens share their session family's evidence. Pagination uses `--offset` (0–256)
and `--limit` (1–100); `--all` follows advancing offsets. JSON and pipelines keep
the complete envelope, including creation provenance, collection status,
retention, source cap, and `overflow_since`. Collection being disabled does not
erase already retained evidence. Activity is best-effort and includes successful
authentication followed by authorization denial; it is not a complete request
ledger or proof that an operation was authorized.

## Rotating sessions and upgrade

Inspect or end an existing session without starting a fresh login:

```sh
hubuum-cli auth status --output json
hubuum-cli auth logout
```

Status reports storage mode, session ID, and access/refresh/absolute deadlines;
it never prints credentials. Outside the REPL, both commands resume only saved
rotating sessions, which may validate or refresh them. A missing or expired
session never prompts for a password. With `--session-storage none`, only an
existing REPL session can be inspected or logged out. Logout revokes that session
and clears only its matching saved family, preserving a newer login from another
process. Explicit token-file configuration is reported without reading the file;
use token revoke commands to revoke PATs and service tokens.

The CLI uses the server v0.0.22 `/api/v1/auth` contract. Password login creates
an automatically rotating session with client ID `hubuum-cli`.

With the default file storage, the CLI saves the access and refresh credentials and their
server deadlines in `sessions.json` under its data directory (on Linux,
`$XDG_DATA_HOME/hubuum_cli`, or `~/.local/share/hubuum_cli`). The directory and
credential files have owner-only permissions on Unix. Passwords are never saved.
Existing cache and lock-file permissions are tightened before use as well.
The cache separates identities by normalized server URL (including protocol,
port, and base path), identity provider, and username.

New CLI processes resume and validate the saved session. This also lets wrapper
commands such as `hubuum-room` reuse one login across their CLI subprocesses.
An open REPL and other CLI processes share the latest credentials through a
`sessions.lock` sidecar. A separate `sessions.login.lock` coordinates cache-miss
logins: after password entry, a process rechecks storage under this lock and
reuses any session created by another process before creating its own. The lock
remains held through login and saving; password prompts never hold either lock.
Updates atomically replace the cache. The refresh
credential is removed durably before exchange and its successor is saved before
use, so a crash or lost refresh response requires a new login instead of replaying
a consumed credential. Server idle and absolute expiry limits still apply.

A revoked session or an expired refresh lifetime requires sign-in again. Network
failures during validation retain the saved session and report the error. A failed
or uncertain refresh discards that session. To forget local sessions, close
running clients and remove `sessions.json`; this does not revoke them at the server.

This restores persistence lost in v0.0.18. Legacy `token.json` entries remain
readable for migration. Explicit `--token-file` authentication bypasses this
session cache and remains suitable for PATs and service accounts.

Upgrade server API, workers, and restore executors together before using this
CLI. The CLI does not downgrade password login to the removed legacy API.

## Disable persistent session storage

Select `none` to keep rotating password-session credentials only in memory:

```sh
hubuum-cli config set --key server.session_storage --value none
# Or select it for one invocation:
hubuum-cli --session-storage none object list
# Or configure the environment, including wrapper subprocesses:
export HUBUUM_CLI__SERVER__SESSION_STORAGE=none
```

Each new CLI process signs in again using the configured password or a prompt.
The REPL still reuses and rotates its session until it exits. Wrappers that start
multiple CLI processes, such as `hubuum-room`, may prompt more than once.

This mode does not open the keychain, read either `sessions.json` or legacy
`token.json`, or create session cache/lock files. It leaves previously saved
credentials intact; selecting `none` does not delete them or revoke server
sessions. Close existing CLI sessions before changing modes. Explicit
`--token-file` authentication still uses the file you provide, and ordinary
configuration, history, and logging behavior is unaffected.

## Optional keychain storage

File storage is the default. On macOS and Linux, explicitly select the operating
system's credential store to keep the access and refresh tokens there:

```sh
hubuum-cli config set --key server.session_storage --value keychain
# Or select it for one invocation:
hubuum-cli --session-storage keychain object list
# Or configure the environment (also inherited by wrapper subprocesses):
export HUBUUM_CLI__SERVER__SESSION_STORAGE=keychain
```

Close existing CLI sessions before changing the storage backend.

macOS uses Keychain. Linux requires a running Secret Service, such as GNOME
Keyring or a compatible KWallet, reachable through the user's D-Bus session.
The store may prompt to unlock or authorize access. SSH sessions need access to
that service; the CLI does not create or unlock a desktop keyring itself.
Unavailable, locked, or unsupported stores report an error. The CLI never
silently switches back to file credentials when keychain storage is selected.
Explicit `--token-file` authentication still bypasses session storage.

The first use moves the matching `sessions.json` entry into the keychain while
holding the shared process lock. It writes the keychain entry successfully before
removing the plaintext entry, and preserves other accounts. A failed migration
stops authentication and can be retried. Subsequent logins and refreshes write
credentials only to the keychain. The `sessions.lock` file remains for process
coordination. Keychain storage does not reuse or migrate legacy `token.json`
credentials; remove obsolete legacy entries separately when no longer needed.
Existing backups or filesystem snapshots may still contain old plaintext copies.

Entries use the service name `org.hubuum.cli.sessions`, scoped by server URL,
provider, username, and canonical data directory. Changing the data directory
uses a different entry and requires login again. Lookup identifiers are metadata,
not secrets, and may be visible in the keychain UI. Passwords are never stored.

Encryption and access control are supplied by Keychain or the Secret Service
provider. This protects credentials at rest according to that provider's policy;
it does not prevent software running as your user from accessing an unlocked
store. Refresh coordination and server expiry limits are unchanged.

To forget a keychain session locally, close running clients and delete its Hubuum
CLI entry using the platform's keychain manager. Local deletion does not revoke
the server session. Selecting `file` again explicitly returns to file storage;
keychain credentials are not exported back into the plaintext cache.

## Authentication project candidate

The `project/auth-support` branch additionally consumes the accepted immutable
Rust SDK auth candidate. Server/SDK/fixture pins are recorded in
[authentication-project-source.json](authentication-project-source.json). The
released v0.0.22 compatibility target stays pinned. These additions require the
accepted auth project server and are not a stable GitHub qualification claim.
Stable promotion requires a released SDK and the separate reviewed milestone.

Discover without logging in, then choose an immutable ID (names are display
metadata):

```bash
hubuum-cli auth methods --output json
hubuum-cli --auth-provider PROVIDER_UUID --no-browser whoami --output json
```

The default browser hook launches the SDK-validated public verification URL only
from an interactive terminal. `--no-browser` explicitly displays URL/code on
stderr for SSH or another browser. Complete the server-owned approval there.
No OIDC/OAuth exchange, identity mapping, upstream tokens or client secrets enter
the CLI. Unknown versions and unsupported capabilities never trigger automatic
password fallback. Existing local/LDAP `--identity-scope`, PAT injection, and
file/keychain/memory storage remain supported. Explicit `--auth-provider local`
or an LDAP ID uses the generic credentials interaction.

Polling is SDK-owned, caller driven, honors backoff/Retry-After and stops on a
terminal state or expiry. Ctrl-C cancels the pending interaction and attempts
exact issued-family cleanup, including ambiguous completion/persistence failure.
Completion is attempted once. The cancellation guard remains active until
persistence or cleanup finishes; a second Ctrl-C force-interrupts. Unknown
mutation/rotation outcomes are never automatically replayed. Keep the existing
serialized session store for process restarts and refresh coordination.

Browser identity comes from Hubuum. The configured username labels a saved
credential slot; it does not select or assert an upstream identity. The REPL
shows `session[PROVIDER_ID]` for this mode; `whoami` shows the actual principal.
Saved slots bind the origin, immutable provider ID and label. Local and external
providers cannot accidentally reuse one another's slot.

### Predictable noninteractive commands

**Breaking behavior:** password prompts are rejected when stdin is not a
terminal. Scripts that previously relied on a controlling terminal while piping
stdin must supply a saved session, protected credential configuration or a token
file. Browser completion requires explicit `--no-browser` in that situation.
Scripts do not silently launch a browser or prompt for a fallback password.

`me token create` fresh-approves an operation for the authenticated principal
without an ambiguous username lookup. Use the selected provider's fresh browser
approval, or explicitly provide the existing owner-only approval password file.
OAuth returns `capability_unsupported`; there is no password downgrade. OIDC
freshness depends on provider policy. Approval binds the original credential and
exact operation through the protected request. Changing credentials cannot rebind
it. One-time issued token output is deliberate credential data, not a diagnostic;
store it in a protected destination. Login/session/proof secrets never appear in
stdout, browser URLs, child arguments or logs.

### Runtime setup for LDAP, OIDC and OAuth

```bash
hubuum-cli admin settings list --output json
hubuum-cli admin auth show --output json
hubuum-cli --no-browser admin auth configure --file /run/secrets/providers.json --output json
hubuum-cli --no-browser --approval-password-file /run/secrets/local-approval admin auth configure --file /run/secrets/providers.json --activate --output json
```

A locally authenticated recovery administrator supplies the complete desired set
and the observed `expected_revision`. Omitting an external provider removes it
from the proposed selection; preserve immutable IDs when editing. Local
authentication cannot be disabled through this workflow. Keep an enabled local
recovery account under the existing IAM controls.

An initial OIDC input looks like this; fill in the deployment's provider details:

```json
{
  "group": "authentication",
  "expected_revision": 1,
  "providers": [
    {
      "name": "employees",
      "kind": "oidc",
      "source": "runtime",
      "enabled": true,
      "configuration": {
        "issuer": "https://issuer.example/realms/company",
        "client_id": "hubuum",
        "client_authentication": "client_secret_basic",
        "scopes": ["openid", "profile"],
        "signing_algorithms": ["RS256"],
        "retain_refresh_tokens": true,
        "membership": {
          "mode": "local_only",
          "refresh_interval_seconds": 300,
          "max_stale_seconds": 900
        },
        "admission": {"mode": "provision_without_grants"},
        "credential": {"action": "replace", "value": "REPLACE_WITH_PRIVATE_SECRET"}
      }
    }
  ]
}
```

Files must be owner-only on Unix (`chmod 600`) and are limited to 1 MiB. `--file -`
reads explicit stdin without placing secrets in process arguments or REPL history.
Use server discovery for complete LDAP/OAuth definitions, admission, provider
groups, claim source/mapping and freshness bounds. Browser credential actions are
`keep`, `clear`, `reference` (an alias) and `replace` (a write-only value). LDAP
omission of `configuration.bind_password` keeps its encrypted value; explicit null
clears it. An enabled LDAP input also includes client-only `verification` with
`username` and `password`, used only for its actual test sign-in. Deployment LDAP
selection/adoption follows the SDK/server ownership rules.

The CLI reports new immutable provider IDs and exact public callback URLs before
test sign-in so the upstream client can be registered. A preview issues no
session, provisions no identity and changes no membership or live login settings.
The default command tests only. `--activate` additionally requires fresh approval
for the exact verified candidate; receipts remain private and never enter output.
A stale revision, receipt, changed secret or deployment lock fails without blind
retry. Refetch effective state and prepare a new candidate. A committed activation
is not an acknowledgement from every replica; inspect `admin auth show` for
application/staleness status and credential-presence flags.

Encryption root keys, verified TLS/egress policy, public browser origin, registered
return targets and deployment locks remain deployment-owned. Runtime credentials
need no per-provider secret environment variable. `--ca-file` adds a trusted PEM
root while preserving certificate validation; do not disable TLS for private test
providers. Effective output excludes stored credentials and aliases.

`group member add/remove --group-id ID --principal-id ID` manages manual grants
for external principals without name collisions. Group `managed_by` and
`identity_scope`, plus the member principal's scope, show ownership. Provider-owned
groups/memberships stay read-only. Independent restored membership-source history
is preserved by the server but is not enumerated by the current read API; group
ownership must not be presented as a complete per-membership provenance history.

### Separate filesystem profiles

Set `HUBUUM_CLI_PROFILE_DIR` to an absolute directory for alternate `system.toml`,
`config.toml`, and `data/` locations. This keeps test configuration, session files,
history and logs away from the normal user profile without changing HOME.
Environment overrides still apply. File and memory storage suit disposable
fixtures; keychain remains the existing identity-scoped backend. The default
locations are unchanged when the variable is absent.
