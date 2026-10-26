# Webhook setup

CLI v0.0.22 uses `hubuum_client` 0.20.0, which targets Hubuum server v0.0.21.
Use `--kind webhook --config` for any receiver that accepts HTTPS JSON POSTs.
The `--target slack|mattermost|discord` presets are shortcuts for this same
generic sink. They generate the configuration and delivery policy described
below; the server stores no provider-specific sink kind or target field.

## Server setup

Use an administrator account to create sinks. Enable delivery workers on the
Hubuum server with `HUBUUM_EVENT_DELIVERY_WORKERS=1`. Fan-out workers must also
be enabled; their default is one. Set worker options on the server and restart
the affected processes.

Receivers need HTTPS with trusted certificates. Private destinations, including
self-hosted receivers and Mattermost installations, require the server's
outbound private-target setting. Redirects are refused. See the server's
[versioned setup guide](https://hubuum.github.io/hubuum/v0.0.21/webhook_notifications/)
for worker settings, secret sources, and network requirements.

## Store the URL on the server

Keep a webhook URL containing credentials in the server's secret source. For
example, the alias `ops_chat_webhook` maps to either:

- Environment source: `HUBUUM_EVENT_SINK_SECRET_OPS_CHAT_WEBHOOK` on every
  delivery worker; restart workers after changing the value.
- File source: `event-sink/ops_chat_webhook` under `HUBUUM_SECRET_FILE_ROOT`,
  containing the complete HTTPS URL without a trailing newline.

Generic configurations select this alias with `config.url_secret_ref`;
presets use `--url-secret-ref`. The CLI option requires `--target` and takes
the alias, never the URL. Alias names allow 1–128 ASCII letters, digits,
underscores, or hyphens. Substitute your own alias in the examples below.

For a receiver requiring bearer authentication, store its token separately in
the same secret source and add `--secret-ref ALIAS` to the generic sink creation
command. The worker sends `Authorization: Bearer TOKEN`. This is independent of
the URL alias; chat presets do not set a bearer token. Neither secret is exposed
to templates or previews.

For Discord, include `?wait=true` in the stored URL (`&wait=true` if it already
has a query). This makes Discord wait for message creation and return HTTP 200.
The CLI cannot inspect or change a URL stored on the server.

## Generic webhooks

Choose either the original event envelope or a custom JSON payload. Both use an
ordinary `webhook` sink and need a subscription before normal events are sent.
An administrator must grant each shared sink to the collection before creating
the subscription. Grants are direct and do not inherit to child collections.

### Send the original event envelope

Store your receiver's full URL under the alias `inventory_webhook`, using the
secret-source mapping above, then create a sink:

```sh
hubuum-cli event sink create --name inventory-hook --kind webhook --config '{"url_secret_ref":"inventory_webhook"}'
hubuum-cli event sink grant --name inventory-hook --collection Inventory
hubuum-cli event subscription create --collection Inventory --sink inventory-hook --name object-changes --entity-types object --actions created,updated
```

With no `body_template`, the server sends the original event envelope as a JSON
POST. With no `response` policy, every HTTP 2xx response succeeds and other
statuses retry with backoff. There is no configured pacing by default, and HTTP
429 has ordinary retry behavior until `response.rate_limit` is enabled.

Leave subscription routing empty when using `url_secret_ref`. For a public URL
without credentials, you can instead put the destination in subscription
routing and omit the URL secret from the sink:

```sh
hubuum-cli event sink create --name public-events --kind webhook --config '{}'
hubuum-cli event sink grant --name public-events --collection Inventory
hubuum-cli event subscription create --collection Inventory --sink public-events --name public-object-changes --entity-types object --actions created,updated --routing '{"url":"https://receiver.example.org/hubuum/events"}'
```

Replace the example URL with your receiver. A subscription's `routing.url`
cannot override a sink's `url_secret_ref`; choose one destination mechanism.

### Send a custom payload

As an alternative to the original-envelope sink, save this configuration as
`inventory-webhook.json`. This example expects HTTP 202 from the receiver:

```json
{
  "url_secret_ref": "inventory_webhook",
  "body_template": "{\"event_id\": {{ event_id | tojson }}, \"message\": {{ (test_marker ~ summary) | tojson }}, \"test\": {{ test | tojson }}}",
  "response": {
    "success_statuses": [202],
    "retry_statuses": [408, 500, 502, 503, 504],
    "rate_limit": true
  }
}
```

Create the sink and subscription, with one-second pacing:

```sh
hubuum-cli event sink create --name inventory-hook --kind webhook --config file://inventory-webhook.json --delivery-policy '{"min_interval_ms":1000}'
hubuum-cli event sink grant --name inventory-hook --collection Inventory
hubuum-cli event subscription create --collection Inventory --sink inventory-hook --name object-changes --entity-types object --actions created,updated
```

Templates use MiniJinja and must render valid JSON. Envelope fields are available
at the top level and the full envelope as `event`. Use `tojson` for dynamic
values. `test` is a boolean; `test_marker` is `[TEST]` followed by a space for tests
and empty for normal delivery. Templates run on the server within its rendering and request
size limits. Delivery remains a JSON POST; templates do not change its method
or credentials.

Match `success_statuses` to your receiver. For an additional JSON acknowledgement,
set `response.body` to `{"kind":"json_equals","pointer":"/ok","value":true}`;
the response must then contain `{"ok":true}` after a successful HTTP status.
A failed acknowledgement is permanent. In this example, only the listed
transient statuses retry; other HTTP failures are permanent, while HTTP 429
defers delivery using `Retry-After` without spending a failure attempt. Missing
or invalid `Retry-After` uses a 60-second cooldown. Transport failures still
retry. See the [server webhook reference](https://hubuum.github.io/hubuum/v0.0.21/events/#configurable-webhook-notifications)
for response rules, custom headers, timeouts, and payload limits.

Delivery is at least once, so a lost acknowledgement can cause duplicates.
Receivers can deduplicate using `X-Hubuum-Event-Id` or an `event_id` included in
the payload. Normal sends use the event UUID as `Idempotency-Key`; queued tests
use a distinct key per test delivery, stable across its retries. Tests also
carry `X-Hubuum-Delivery-Purpose: test`.

## Choose a destination

| Target | Create the incoming webhook | Message and acknowledgement |
| --- | --- | --- |
| `slack` | Slack app settings → Incoming Webhooks → Add New Webhook to Workspace | JSON `text`; HTTP 200 and trimmed body `ok` |
| `mattermost` | Integrations → Incoming Webhooks; select the channel | JSON `text`; HTTP 200 and trimmed body `ok` |
| `discord` | Server Settings → Integrations → Webhooks; use a regular text channel | JSON `content`; HTTP 200 with `wait=true` |

Slack chooses the channel and identity in its webhook settings. Mattermost
channel and identity overrides depend on server policy; the preset uses the
configured channel. Discord forum and media channels need additional thread
settings and a custom configuration.

See the provider setup guides for [Slack](https://docs.slack.dev/messaging/sending-messages-using-incoming-webhooks/),
[Mattermost](https://docs.mattermost.com/integrations-guide/incoming-webhooks),
and [Discord](https://support.discord.com/hc/en-us/articles/228383668-Intro-to-Webhooks).

## Create the sink and subscription

Use an administrator account to create a sink. Select one command for your
destination, substituting your alias:

```sh
hubuum-cli event sink create --name ops-chat --target slack --url-secret-ref ops_chat_webhook
hubuum-cli event sink create --name ops-chat --target mattermost --url-secret-ref ops_chat_webhook
hubuum-cli event sink create --name ops-chat --target discord --url-secret-ref ops_chat_webhook
```

The same commands work in the REPL without `hubuum-cli`. Tab completes target
names; `help event sink create` explains the setup. Sink creation alone sends
no message. Grant the shared sink to the collection as an administrator, then
select events with a subscription in a collection you manage:

```sh
hubuum-cli event sink grant --name ops-chat --collection Inventory
hubuum-cli event subscription create --collection Inventory --sink ops-chat --name object-changes --entity-types object --actions created,updated
hubuum-cli event sink show ops-chat
```

Leave subscription routing empty: `url_secret_ref` supplies the destination,
and a `routing.url` override is rejected by the server. Reuse one sink for
subscriptions sharing a channel so they share pacing.

Presets use the server's summary and test marker, escape values with `tojson`,
retry HTTP 408/500/502/503/504, and honor HTTP 429 cooldowns. Discord messages
are limited to 1,900 characters and disable automatic mentions. Slack and
Mattermost use the server's ordinary summary without a provider-specific length
limit; use a custom template if your messages need truncation or rich formatting.

## What the presets configure

| Setting | Generic webhook default | Slack / Mattermost preset | Discord preset |
| --- | --- | --- | --- |
| Kind | `webhook` | `webhook` | `webhook` |
| Destination | `config.url_secret_ref` or subscription `routing.url` | URL secret alias from `--url-secret-ref` | URL secret alias from `--url-secret-ref`; stored URL must include `wait=true` |
| Payload | Original event envelope | JSON `text`: test marker, `Hubuum:` prefix, summary | JSON `content`: same message, truncated to 1,900 characters; automatic mentions disabled |
| Success | Any HTTP 2xx | HTTP 200 and trimmed body `ok` | HTTP 200; no body check |
| HTTP retries | All non-2xx statuses | 408, 500, 502, 503, 504 | 408, 500, 502, 503, 504 |
| HTTP 429 | Ordinary retry | Provider cooldown | Provider cooldown |
| Configured pacing | None | 1,000 ms | 1,000 ms |
| Enabled | `true` | `true` | `true` |

`--enabled false` creates a disabled sink, and `--delivery-policy` overrides the
preset's default pacing. A preset requires a URL alias and rejects `--config`,
`--secret-ref`, or a non-webhook `--kind`. Use the generic form to customize any
of those settings.

The shortcuts create only the sink. You still provision the provider webhook,
store its URL on the server, enable workers, create subscriptions, and verify
delivery. They do not rewrite Discord URLs or send a setup/test message.

### Full Slack and Mattermost commands

Save this as `chat-webhook.json`:

```json
{
  "url_secret_ref": "ops_chat_webhook",
  "body_template": "{\"text\": {{ (test_marker ~ 'Hubuum: ' ~ summary) | tojson }}}",
  "response": {
    "success_statuses": [200],
    "rate_limit": true,
    "retry_statuses": [408, 500, 502, 503, 504],
    "body": {"kind": "text_equals", "value": "ok"}
  }
}
```

Either shorthand below produces the same request as the full command. Choose
one command, with the alias resolving to the chosen provider's webhook URL:

```sh
hubuum-cli event sink create --name ops-chat --target slack --url-secret-ref ops_chat_webhook
hubuum-cli event sink create --name ops-chat --target mattermost --url-secret-ref ops_chat_webhook
hubuum-cli event sink create --name ops-chat --kind webhook --config file://chat-webhook.json --delivery-policy '{"min_interval_ms":1000}' --enabled true
```

### Full Discord command

Save this as `discord-webhook.json`:

```json
{
  "url_secret_ref": "ops_chat_webhook",
  "body_template": "{\"content\": {{ (test_marker ~ 'Hubuum: ' ~ summary)[:1900] | tojson }}, \"allowed_mentions\": {\"parse\": []}}",
  "response": {
    "success_statuses": [200],
    "rate_limit": true,
    "retry_statuses": [408, 500, 502, 503, 504]
  }
}
```

These are equivalent alternatives; both require `wait=true` in the server-held
URL:

```sh
hubuum-cli event sink create --name ops-chat --target discord --url-secret-ref ops_chat_webhook
hubuum-cli event sink create --name ops-chat --kind webhook --config file://discord-webhook.json --delivery-policy '{"min_interval_ms":1000}' --enabled true
```

## Adjust and verify

All presets start with one-second spacing. Override it at creation with
`--delivery-policy '{"min_interval_ms":2000}'`, or update an existing sink:

```sh
hubuum-cli event sink update --sink ops-chat --delivery-policy '{"min_interval_ms":2000}'
hubuum-cli event sink update --sink ops-chat --delivery-policy '{}'
```

The second command removes configured pacing. Nonempty intervals must be
1–86,400,000 milliseconds. Provider cooldowns still apply when enabled in the
response policy. To customize a sink created by either setup path, edit a full
configuration file and replace the saved configuration:

```sh
hubuum-cli event sink update --sink ops-chat --config file://chat-webhook.json
```

Configuration is replaced as a whole. Retain `url_secret_ref`, `body_template`,
and `response` if you want to keep their behavior. Omitting the template restores
the original event envelope; omitting the response rules restores generic HTTP
handling. Updating only `--config` leaves the separate delivery policy unchanged.

## Preview and test notifications

Administrators can render a saved event and explicitly queue a real test:

```sh
hubuum-cli audit list --output json
hubuum-cli event subscription show --collection Inventory --name inventory-changes
hubuum-cli event sink preview --name ops-slack --subscription-id 7 --event 11111111-1111-4111-8111-111111111111
hubuum-cli event sink test --name ops-slack --subscription-id 7 --event 11111111-1111-4111-8111-111111111111
hubuum-cli event delivery show 123
```

Substitute the subscription's numeric `id` and the audit event's `event_id` UUID;
the numeric audit row `id` is a different identifier. Preview renders without
looking up secrets or sending a message. Test queues a real delivery, even when
the sink or subscription is disabled or the event does not match its filters.
Both operations require administrator access and retain server scope checks.
Rendered payloads can contain sensitive event data. Preset templates display
`[TEST]`; a generic event envelope has no automatically inserted message label.

Test returns the delivery record. Inspect its ID with `event delivery show` and
verify it reaches `succeeded` and appears at the receiver. Delivery workers,
credentials, and pacing still apply. After an ambiguous submission failure,
inspect `event delivery list` before issuing another test; mutations are not
replayed automatically.

## System subscriptions and login alerts

Use exactly one of `--system` or `--collection` with every subscription command.
System subscriptions require administrator access and use global sink lookup.
For successful administrator logins:

```sh
hubuum-cli event subscription create --system --name admin-logins --sink ops-slack --entity-types authentication --actions succeeded --filter '{"subject_identity_scopes":["local"],"subject_is_admin":true}'
hubuum-cli event subscription list --system
hubuum-cli event subscription show --system --name admin-logins
hubuum-cli event subscription update --system --subscription admin-logins --enabled false
hubuum-cli event subscription delete --system --name admin-logins
```

Filters also accept `subject_principal_ids`. They select the immutable subject
captured when the authentication event occurred, independently of the event's
actor. Audit JSON retains that subject under `metadata.subject`. `--filter {}`
clears subject filters. System list returns the complete list and accepts
`--all` and `--include-total`; server filtering, sorting, limits, and cursors are
unsupported. Use local output pipelines to filter or sort it. REPL completion
includes system subscription names and the `authentication` event type.

To inspect relationship activity involving a resource, including deleted relations:

```sh
hubuum-cli audit resource --resource object --class Hosts --name host-01 --include-relations --all
hubuum-cli audit resource --resource class --name Hosts --include-relations
```

Without `--include-relations`, audit results retain their entity-only behavior.
The option requires a class or object; the server still applies audit permissions
and snapshot redaction.

The pinned integration suite checks all three chat previews, system subscription
CRUD, login filters, queued test delivery, and relation-inclusive audit. It does
not send messages to hosted providers or verify your channel credentials.

## Collection-owned destinations

With Hubuum server `v0.0.18` and Rust client
`0.14.1`, a collection manager can configure a destination and subscription:

```text
event sink create --collection Inventory --name alerts --target slack --destination-url-file slack-webhook-url.txt
event sink list --collection Inventory
event subscription create --collection Inventory --sink alerts --name changes --entity-types object --actions updated
```

The URL file contains the complete HTTPS webhook URL. Keep it private. Presets
require file input so the URL does not enter process arguments or REPL history. For a custom
message, pass `--kind webhook --config file://webhook.json` with a fixed
`destination_url` in the configuration. Creation and editing require
`ManageEventSubscription` and `ReadAudit`; listing and deletion require management.
Saved URLs are omitted from collection results. Supply replacement configuration
to rotate a URL, and remove subscriptions before deleting their destination.
Destinations belong to the collection and survive the creator losing access.

An administrator can grant a global sink with
`event sink grant --name shared --collection Inventory`, inspect grants with
`event sink collections --name shared`, or revoke with
`event sink revoke --name shared --collection Inventory`. Grants do not inherit.
Delivery diagnostics and retries remain administrator operations.

The required integration command runs the delegated workflow on its own disposable
server, followed by administrator, backup, and restore checks:

```sh
cargo build --locked
python3 scripts/test-backup-restore.py
```

For a focused check against an existing disposable server, run
`python3 scripts/test-collection-integrations.py` with `HUBUUM_E2E_BASE_URL` and
`HUBUUM_E2E_ADMIN_PASSWORD`. Set `HUBUUM_E2E_CLI_BINARY` when the executable is
outside `target/debug`. The script creates and removes its own user, group, and
collection fixtures.
