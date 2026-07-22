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
    "collected_at": "2026-07-22T02:15:00Z",
    "os": {
      "kernel": {
        "name": "Linux",
        "release": "6.14.11-300.fc42.x86_64"
      },
      "fedora": {
        "client": true,
        "major": "42",
        "type": "Client",
        "version": "42"
      }
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

## Expected Patch Format

The value supplied with `--patch` must be an RFC 6902 JSON Patch document. The
file is UTF-8 JSON whose top-level value is an array of patch operations. It is
the request body only: do not wrap it in an object containing a URL, HTTP method,
headers, or `body` member. Hubuum CLI selects the PATCH endpoint and sends the
`application/json-patch+json` content type.

For normal Ansible publication, use exactly one operation: `add` the complete
normalized snapshot at `/facts`. The following is a representative patch file:

```json
[
  {
    "op": "add",
    "path": "/facts",
    "value": {
      "schema_version": 1,
      "source": "ansible",
      "collected_at": "2026-07-22T02:15:00Z",
      "network": {
        "interfaces": [
          {
            "name": "ens192",
            "ipv4": "129.240.1.10",
            "ipv6": "2001:db8::10",
            "mac": "00:11:22:33:44:55"
          }
        ]
      },
      "os": {
        "kernel": {
          "name": "Linux",
          "release": "6.14.11-300.fc42.x86_64"
        },
        "fedora": {
          "client": true,
          "major": "42",
          "type": "Client",
          "version": "42"
        }
      },
      "hardware": {
        "serial": "HOST-1",
        "model": "PowerEdge R650",
        "cpu": {
          "summary": "8 x AMD EPYC 7262"
        },
        "memory": {
          "total": "16 GB"
        }
      },
      "observed": {
        "uptime": 123456,
        "last_topuser": "alice"
      }
    }
  }
]
```

The normalized `/facts` value has this version 1 contract:

| Member | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Required; currently `1` |
| `source` | string | Required; `ansible` for this publisher |
| `collected_at` | string | Required RFC 3339 UTC timestamp |
| `network.interfaces` | array | Observed interfaces with a required `name` and optional string `ipv4`, `ipv6`, and `mac` members |
| `os.kernel` | object | Optional `name` and `release` strings |
| `os.redhat` | object | RHEL `version`, `major`, and `type` strings plus `client` and `server` booleans when known |
| `os.fedora` | object | Fedora `version`, `major`, and `type` strings plus a `client` boolean when known |
| `os.macos` | object | macOS `version` string |
| `hardware` | object | Optional `serial` and `model` strings, `cpu.summary` string, and `memory.total` string |
| `observed` | object | Volatile observations such as integer `uptime` seconds and string `last_topuser` |

Include at most one of `os.redhat`, `os.fedora`, or `os.macos` in a snapshot.
Omit values that Ansible did not observe; do not encode missing values as empty
strings or the strings `null`, `true`, or `false`. Version and major-release
values are strings because they are identifiers, while flags are JSON booleans.
Additional version 1 members require a reviewed contract change rather than
copying arbitrary keys from `ansible_facts`.

RFC 6902 `add /facts` creates the member when absent and replaces its complete
value when present. It is not a recursive merge. Replacing the complete snapshot
therefore removes RHEL-only values after a machine changes to Fedora while
preserving unrelated top-level data.

An existing object may instead be updated with a compare-and-set patch containing
`test /facts` with the exact previously read snapshot followed by `add /facts`
with the new snapshot. A failed `test` rejects the whole patch. This form requires
a prior read and must not be used with `--create`, because `/facts` cannot be
tested on a new empty object.

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
