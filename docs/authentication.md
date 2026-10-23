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

The CLI uses the server v0.0.20 `/api/v1/auth` contract. Password login creates
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
