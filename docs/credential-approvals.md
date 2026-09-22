# Fresh credential approvals

Hubuum server v0.0.16 requires a fresh password approval for local user creation,
password changes, token creation/renewal (including CLI token cloning), imports
that contain credentials even in dry-run mode, and restore confirmation.
Existing bearer-only automation for these operations must change when upgrading.

## Interactive use

Run the usual command with an unscoped human bearer. When the server returns
`reauthentication_required`, the CLI describes the operation and prompts for the
acting human's current password without echoing it. This is the approver's
password, not the new user's password or the target account's password. Login
passwords saved in configuration or environment variables are not reused.

A service-account bearer or a token restricted by permission scopes cannot
approve credential operations. An identity-provider scope is different from a
token permission scope and is allowed. Normal server authorization still applies.

The CLI binds the approval to the operation, payload, and original bearer,
then sends the approved operation once. Token expiry returned by the approval
endpoint is retained exactly. Approval lifetime is at most 120 seconds and the
approval is single-use. The CLI does not persist approval secrets.

## Scripts and one-shot commands

Provide the acting human's current password in a private regular file and pass
the global option before the command:

```sh
chmod 600 /run/secrets/hubuum-approval-password
hubuum-cli --token-file /run/secrets/hubuum-human-token --approval-password-file /run/secrets/hubuum-approval-password user token create --username alice --name inventory
hubuum-cli --approval-password-file /run/secrets/hubuum-approval-password restore confirm --receipt restore.json --yes --wait
```

On Unix, files with group or other permissions are rejected. On other platforms,
use appropriate directory/file access controls. The file is read afresh for each
protected operation, is limited to 16 KiB, and may end with a single newline.
Meaningful spaces are preserved. This option is not saved in CLI configuration.
Scripts and noninteractive stdin never fall back to a password prompt; they fail
with instructions when approval is required and no password file was supplied.

`--yes` for restore confirmation still records destructive intent and does not
replace password approval. Full import graphs now preserve supported credential
and integration entries. Review files whose extended entries may previously
have been ignored before resubmitting them. `--collection` also rewrites the
collection references of imported templates, remote targets, and subscriptions.

## Evidence and failures

Before sending an approved mutation, the CLI prints the non-secret approval ID
to stderr. Preserve that ID when investigating a lost response:

```sh
hubuum-cli auth approval show 123 --output json
```

This reads server-retained evidence and does not expose the approval secret.
Approval authentication failures, expired/consumed approvals, and ambiguous
mutation failures do not trigger login or automatic replay. Inspect both the
approval record and the target resource before retrying: the operation may have
committed even if its response was lost. For restores, also inspect the receipt
with `restore status`. Requesting another approval is a new operation attempt.

For the server migration and pinned verification evidence, see
[compatibility](../COMPATIBILITY.md) and [backup recovery](backup-restore.md).
