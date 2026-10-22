# Client and server compatibility

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
Each CLI release pins a `hubuum_client` version; that client library declares
and reproducibly tests its Hubuum server target. CLI v0.0.18 uses
`hubuum_client` 0.16.0, which targets Hubuum server v0.0.20.
The CLI inherits this compatibility target from its client dependency and runs
its own integration checks against the same pinned server. Command availability
against other server versions is not guaranteed.

## Compatibility matrix

| CLI version | `hubuum_client` dependency | Client's Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.18 | 0.16.0 | 0.0.20 | Rotating interactive sessions and backup format 10 |
| 0.0.17 | 0.15.1 | 0.0.19 | Previous release; structured JSON command diagnostics |
| 0.0.16 | 0.15.1 | 0.0.19 | Previous CLI release; command tracing and adaptive grouped table headers |
| 0.0.15 | 0.15.0 | 0.0.19 | Previous CLI release; query usage, system notifications, structured streaming, backup output 9 and restore input 6/7/8/9 |
| 0.0.14 | 0.14.1 | 0.0.18 | Previous CLI release; collection-owned integrations, direct sink grants, backup output 8 and restore input 6/7/8 |
| 0.0.13 | 0.13.0 | 0.0.17 | Previous CLI release; chat webhook presets, notification policies, backup output 7 and restore input 6/7 |
| 0.0.12 | 0.12.0 | 0.0.16 | Previous CLI release; structured search, credential approvals, task discovery, and backup format 6 |
| 0.0.11 | 0.10.1 | 0.0.14 | Previous CLI release; backup format 5, queued restores, and restorable follow-up backups |
| 0.0.10 | 0.9.1 | 0.0.9 | Previous CLI release |
| 0.0.9 | 0.9.0 | 0.0.9 | Previous CLI release |
| 0.0.8 | 0.8.0 | 0.0.8 | Previous CLI release |
| 0.0.5 | 0.7.2 | 0.0.5 | Previous CLI release |
| 0.0.4 | 0.7.1 | 0.0.4 | Previous CLI release |
| 0.0.3 | 0.6.1 | 0.0.3 | Previous CLI release |
| 0.0.2 | 0.5.1 | 0.0.2 | Previous CLI release |
| 0.0.1 | 0.4.0 | `main@eed194f2339ce221ef251a14062e2a37850186b1` | Historical pre-release snapshot; no stable server target was declared |

## Before upgrading

CLI v0.0.18 targets server v0.0.20 through client 0.16.0. Upgrade server API,
workers, and restore executors together before using the new versioned login.
New password sessions remain in memory and rotate automatically. Each new CLI
process needs credentials; use an explicit PAT file for unattended automation.
See [authentication](docs/authentication.md) and
[credential approvals](docs/credential-approvals.md).

The server requires an offline migration and emits backup format 10. Keep a
pre-upgrade PostgreSQL snapshot and matching binaries for rollback. Restore
accepts formats 6–10; older servers cannot restore the new output. Follow the
[server release notes](https://github.com/hubuum/hubuum/releases/tag/v0.0.20)
and [backup compatibility](https://hubuum.github.io/hubuum/v0.0.20/backup-restore/).

**Breaking for extension authors in v0.0.16:** rename options named `debug` or
aliased as `--debug`; the host now reserves this flag for command diagnostics.

## Verification evidence

[Detailed evidence and historical migrations](docs/compatibility-evidence.md) retain tested
image identities, dates, suite results, and earlier compatibility limits.

## CLI v0.0.18: client v0.16.0

See [CLI v0.0.18 evidence](docs/compatibility-evidence.md#cli-v0018-client-v0160)
for the published SDK checksum, immutable server image, session behavior, and
completed workspace and live recovery checks.

## CLI v0.0.17: client v0.15.1

JSON debug consumers must now read command results from `.data` and diagnostics
from `.debug`. The client dependency and server target remain unchanged. See
[CLI v0.0.17 evidence](docs/compatibility-evidence.md#cli-v0017-client-v0151).

## CLI v0.0.16: client v0.15.1

See [CLI v0.0.16 evidence](docs/compatibility-evidence.md#cli-v0016-client-v0151)
for the tracing contract, pinned server identity, and completed checks.

## CLI v0.0.15: client v0.15.0

See [CLI v0.0.15 evidence](docs/compatibility-evidence.md#cli-v0015-client-v0150)
for the pinned server identity and completed checks.

## CLI v0.0.14: client v0.14.1

See [cli v0.0.14: client v0.14.1](docs/compatibility-evidence.md#cli-v0014-client-v0141) in the historical evidence.

## CLI v0.0.13: client v0.13.0

See [cli v0.0.13: client v0.13.0](docs/compatibility-evidence.md#cli-v0013-client-v0130) in the historical evidence.

## CLI v0.0.12: client v0.12.0

See [cli v0.0.12: client v0.12.0](docs/compatibility-evidence.md#cli-v0012-client-v0120) in the historical evidence.

## CLI v0.0.11: client v0.10.1

See [cli v0.0.11: client v0.10.1](docs/compatibility-evidence.md#cli-v0011-client-v0101) in the historical evidence.
