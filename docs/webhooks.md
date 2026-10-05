# Chat webhook setup

CLI v0.0.13 targets Hubuum server v0.0.17. Use a target preset to create an
ordinary `webhook` sink for Slack, Mattermost, or Discord. The target is only
a CLI setup choice: the server stores the generated configuration and delivery
policy, with no provider-specific sink kind. Existing custom sinks keep working.

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

## Store the URL on the server

Keep the entire HTTPS webhook URL in the server's secret source. For example,
the alias `ops_chat_webhook` maps to either:

- Environment source: `HUBUUM_EVENT_SINK_SECRET_OPS_CHAT_WEBHOOK` on every
  delivery worker; restart workers after changing the value.
- File source: `event-sink/ops_chat_webhook` under `HUBUUM_SECRET_FILE_ROOT`,
  containing the URL without a trailing newline.

For Discord, include `?wait=true` in the stored URL (`&wait=true` if it already
has a query). This makes the server wait for message creation and receive HTTP
200. The CLI cannot inspect or change a URL stored on the server.

`--url-secret-ref` takes the alias, never the URL. The separate `--secret-ref`
option is for bearer authentication on custom sinks; chat webhook presets do
not use it. Alias names allow 1–128 ASCII letters, digits, underscores, or hyphens.

Enable delivery workers on the Hubuum server with
`HUBUUM_EVENT_DELIVERY_WORKERS=1`. Fan-out workers must also be enabled; their
default is one. Private Mattermost destinations require the server's outbound
private-target setting and trusted HTTPS certificates. See the server's
[versioned setup guide](https://hubuum.github.io/hubuum/v0.0.17/webhook_notifications/)
for secret sources, worker settings, and network requirements.

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
no message. Select events with a subscription in a collection you manage:

```sh
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

## Adjust and verify

All presets start with one-second spacing. Override it at creation with
`--delivery-policy '{"min_interval_ms":2000}'`, or update an existing sink:

```sh
hubuum-cli event sink update --sink ops-chat --delivery-policy '{"min_interval_ms":2000}'
hubuum-cli event sink update --sink ops-chat --delivery-policy '{}'
```

The second command removes configured pacing. Nonempty intervals must be
1–86,400,000 milliseconds. Provider cooldowns still apply. For custom payloads,
use `event sink update --config file://webhook-config.json`; configuration is
replaced as a whole, so include `url_secret_ref`, `body_template`, and `response`.
To create a custom sink, use `--kind webhook --config file://webhook-config.json`
instead of `--target`.

Use the server's [preview and test steps](https://hubuum.github.io/hubuum/v0.0.17/webhook_notifications/#preview-test-and-check-delivery)
before enabling production notifications. Preview renders a saved event without
looking up secrets or sending a message; test queues a real `[TEST]` message.
These endpoints and system-subscription CRUD currently require the server API;
this release has no dedicated CLI commands for them. Verify the delivery reaches
`succeeded` and the message appears in your channel. Queuing is not delivery.

The pinned CLI integration test creates each preset and checks real server
previews and pacing round trips. It does not send messages to hosted providers
or verify your channel permissions and credentials.
