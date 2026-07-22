# Publishing Ansible Facts to Hubuum

Hubuum CLI can patch an object's JSON data through exact class and object names,
without preliminary ID lookups. Ansible facts are one possible use case for this
interface.

Hubuum CLI does not define a schema for object data and does not reserve a
`facts` member. The paths and values in this guide are examples. The consumer
decides what to publish, where to store it, and whether individual fields or a
complete subtree should be replaced.

## JSON Patch Input

The value supplied with `--patch` must be a JSON Patch document conforming to
[RFC 6902](https://www.rfc-editor.org/rfc/rfc6902). It may be supplied as inline
JSON, read from `@FILE`, or read with the existing `file://FILE` value-source
form.

The resolved document must be UTF-8 JSON with an array as its top-level value.
Each array element is one patch operation. Supply the patch document itself, not
an envelope containing a URL, HTTP method, headers, or `body` member. Hubuum CLI
selects the endpoint and sends the document with the
`application/json-patch+json` content type.

RFC 6902 defines these operation shapes:

| Operation | Required members |
| --- | --- |
| `add` | `op`, `path`, `value` |
| `remove` | `op`, `path` |
| `replace` | `op`, `path`, `value` |
| `move` | `op`, `from`, `path` |
| `copy` | `op`, `from`, `path` |
| `test` | `op`, `path`, `value` |

The `path` and `from` values are JSON Pointers as defined by
[RFC 6901](https://www.rfc-editor.org/rfc/rfc6901). The empty string addresses
the document root. Within a pointer token, encode `~` as `~0` and `/` as `~1`.
Operations are evaluated in array order. If an operation fails, the patch fails.

For example, this is a complete patch document:

```json
[
  {
    "op": "add",
    "path": "/facts",
    "value": {
      "source": "ansible",
      "distribution": "Fedora"
    }
  }
]
```

Here, `facts`, `source`, and `distribution` are application choices, not Hubuum
CLI fields. At an object member, RFC 6902 `add` creates the member when absent and
replaces its value when present. The target's parent must already exist.

## Example: Replacing a Publisher-Owned Subtree

A publisher may choose to replace one complete subtree on each run. For example,
suppose it first sends:

```json
[
  {
    "op": "add",
    "path": "/facts",
    "value": {
      "distribution": "RHEL",
      "rhel_subscription": "active"
    }
  }
]
```

It can later send:

```json
[
  {
    "op": "add",
    "path": "/facts",
    "value": {
      "distribution": "Fedora"
    }
  }
]
```

The second standard `add` operation replaces the complete value at `/facts`, so
`rhel_subscription` is no longer present. Other top-level object data is
unchanged. This is one way a consumer can clear values that it no longer emits;
consumers may instead use any other valid JSON Patch strategy.

## Creating a Missing Object

The normal flow sends one exact-name PATCH request. With `--create`, a 404 causes
Hubuum CLI to apply the supplied patch locally to an empty JSON object, `{}`, and
send the resulting data to the exact class-name create endpoint.

Consequently, a patch used with `--create` must be capable of initializing an
empty object. The example `add /facts` patch does so. An initial `replace`,
`remove`, or `test` of `/facts` does not, because that member is absent. An `add`
to `/facts/os` also fails on an empty object unless an earlier operation creates
the `/facts` parent. If local application fails, no create request is sent.

If another publisher creates the same object between PATCH and create, Hubuum
returns a conflict. Hubuum CLI retries the exact-name PATCH once. The retry is
bounded; every other error is returned immediately.

## Automation Example

An Ansible controller could invoke the CLI as follows:

```sh
hubuum-cli \
  --hostname hubuum.example.org \
  --protocol https \
  --port 443 \
  --token-file /run/secrets/hubuum-publisher.token \
  object data patch \
  --class Hosts \
  --name srv-01.example.org \
  --patch @facts-patch.json \
  --create \
  --description "Managed by Ansible"
```

The token file contains only the raw bearer token. It may also be selected with
`HUBUUM_CLI__SERVER__TOKEN_FILE`. The `--description` value is used only if the
object is created.

This flow requires no preliminary reads. A service account that uses `--create`
needs `CreateObject` and `UpdateObject` on the collection containing the target
class. Without `--create`, only `UpdateObject` is needed. `ReadObject` can be
granted separately if the consumer needs reads or diagnostics.
