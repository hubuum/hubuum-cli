# Client and server compatibility

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
Each CLI release pins a `hubuum_client` version; that client library declares
and reproducibly tests its Hubuum server target. CLI v0.0.14 uses
`hubuum_client` 0.14.1, which targets Hubuum server v0.0.18.
The CLI inherits this compatibility target from its client dependency and runs
its own integration checks against the same pinned server. Command availability
against other server versions is not guaranteed.

## Compatibility matrix

| CLI version | `hubuum_client` dependency | Client's Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.14 | 0.14.1 | 0.0.18 | Current target; collection-owned integrations, direct sink grants, backup output 8 and restore input 6/7/8 |
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

CLI v0.0.14 targets server v0.0.18 through client 0.14.1. Protected credential
mutations and restore confirmation require fresh human approval. Use
[credential approvals](docs/credential-approvals.md), [webhooks](docs/webhooks.md),
and [backup and restore](docs/backup-restore.md) for command-specific behavior.

Server v0.0.18 requires an offline upgrade from v0.0.17 and emits backup format 8;
older servers cannot restore that format. Follow the canonical
[server upgrade and recovery instructions](https://hubuum.github.io/hubuum/v0.0.18/events/#upgrade-and-rollback)
and [backup compatibility](https://hubuum.github.io/hubuum/v0.0.18/backup-restore/).
Use the server guide for the actual version transition rather than copying
migration commands from an older client release.

## Verification evidence

[Detailed evidence and historical migrations](docs/compatibility-evidence.md) retain tested
image identities, dates, suite results, and earlier compatibility limits.

## CLI v0.0.14: client v0.14.1

See [cli v0.0.14: client v0.14.1](docs/compatibility-evidence.md#cli-v0014-client-v0141) in the historical evidence.

## CLI v0.0.13: client v0.13.0

See [cli v0.0.13: client v0.13.0](docs/compatibility-evidence.md#cli-v0013-client-v0130) in the historical evidence.

## CLI v0.0.12: client v0.12.0

See [cli v0.0.12: client v0.12.0](docs/compatibility-evidence.md#cli-v0012-client-v0120) in the historical evidence.

## CLI v0.0.11: client v0.10.1

See [cli v0.0.11: client v0.10.1](docs/compatibility-evidence.md#cli-v0011-client-v0101) in the historical evidence.
