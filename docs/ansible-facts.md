# Publishing Ansible Facts to Hubuum

Hubuum CLI can publish a complete fact snapshot atomically without first looking
up class or object IDs. Use the exact-name object-data PATCH route and keep
Ansible-owned observations under one top-level `facts` member.

## Data Ownership

Keep values with different owners in separate top-level members:

```json
{
  "inventory": {
    "owner": "platform",
    "lifecycle": "production"
  },
  "facts": {
    "schema_version": 1,
    "source": "ansible",
    "collected_at": "2026-07-21T12:00:00Z",
    "os": {
      "family": "RedHat",
      "distribution": "Fedora",
      "version": "42"
    }
  }
}
```

The importer should place Miami-derived observations under `facts` as well.
Curated metadata and relationships remain outside that member. The first
successful Ansible publication then replaces the imported snapshot without
touching manually maintained values.

Prefer a normalized, reviewed subset of Ansible facts over publishing the entire
`ansible_facts` object. This keeps the data contract stable and avoids collecting
unexpected sensitive or high-volume values.

## Patch Document

Build one RFC 6902 document after fact gathering has completed successfully:

```json
[
  {
    "op": "add",
    "path": "/facts",
    "value": {
      "schema_version": 1,
      "source": "ansible",
      "collected_at": "2026-07-21T12:00:00Z",
      "os": {
        "family": "RedHat",
        "distribution": "Fedora",
        "version": "42"
      }
    }
  }
]
```

RFC 6902 `add /facts` creates the member when absent and replaces its complete
value when present. It is not a recursive merge. Replacing the complete snapshot
therefore removes RHEL-only values after a machine changes to Fedora while
preserving unrelated top-level data.

Publish the document with one command delegated to the Ansible controller:

```sh
hubuum-cli \
  --hostname hubuum.example.org \
  --protocol https \
  --port 443 \
  --token-file /run/secrets/hubuum-facts.token \
  object data patch \
  --class Hosts \
  --name srv-01.example.org \
  --patch @facts-patch.json \
  --create \
  --description "Managed by Ansible"
```

The token file contains only the raw bearer token. Make it readable only by the
automation account and avoid copying it to managed hosts. It may also be selected
with `HUBUUM_CLI__SERVER__TOKEN_FILE`.

## Missing Objects and Concurrency

The normal flow is one exact-name PATCH request. With `--create`, a 404 causes
Hubuum CLI to apply the patch locally to an empty JSON object and send that result
to the exact class-name create endpoint. If the patch cannot initialize an empty
object, no create request is sent and the command reports an actionable error.

If another controller creates the same Host between PATCH and create, Hubuum
returns a conflict. Hubuum CLI retries the exact-name PATCH once. The retry is
bounded; every other error is returned immediately.

This flow requires no preliminary reads. The publishing service account needs
`CreateObject` and `UpdateObject` on the collection containing `Hosts`.
`ReadObject` can be granted separately for diagnostics but is not required for
publication.

## Ansible Execution Rules

- Gather and normalize all facts before constructing the patch document.
- Publish once per host only after the complete gathering block succeeds.
- Run the Hubuum CLI command on the controller with `delegate_to: localhost`.
- Keep the token in Ansible Vault or another secret store and materialize it as an
  owner-only temporary file on the controller.
- Treat a nonzero CLI exit status as a failed publication; do not send a reduced
  or partially gathered snapshot as a fallback.
- Include `collected_at`, `source`, and `schema_version` so consumers can detect
  stale data and evolve the fact contract deliberately.
