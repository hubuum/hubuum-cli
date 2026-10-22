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

## Rotating sessions and upgrade

CLI v0.0.18 uses Rust SDK 0.16.0 and the server v0.0.20 `/api/v1/auth` contract.
Password login creates a session with client ID `hubuum-cli`. The SDK rotates
access and refresh credentials automatically before API requests; the REPL and
scripts reuse the same in-memory session. The server configures access, idle,
and absolute lifetimes. Refresh failure or uncertain exchange requires sign-in.

**Breaking:** new interactive sessions are not written to `token.json`.
Every new CLI process therefore needs a password (prompted or configured), or an
explicit PAT/service token file. Existing cache entries are read for migration
and validated by the server; invalid old interactive tokens require sign-in.
No refresh secret is persisted or shared between processes. Use the REPL for
interactive reuse and an owner-only `--token-file` for unattended automation.

Upgrade server API, workers, and restore executors together before using this
CLI. The CLI does not downgrade password login to the removed legacy API.
