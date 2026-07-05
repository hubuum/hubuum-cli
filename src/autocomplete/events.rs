use crate::services::CompletionContext;

pub fn event_sinks(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.event_sinks(prefix)
}

pub fn event_subscriptions(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    ctx.event_subscriptions_from_namespace(prefix, parts)
}

pub fn event_sink_kinds(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(&["webhook", "amqp", "valkey_stream", "email"], prefix)
}

pub fn event_entity_types(
    _ctx: &CompletionContext,
    prefix: &str,
    _parts: &[String],
) -> Vec<String> {
    complete_csv_values(
        &[
            "namespace",
            "class",
            "object",
            "group",
            "user",
            "report_template",
            "remote_target",
            "task",
            "event_sink",
            "event_subscription",
        ],
        prefix,
    )
}

pub fn event_actions(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_csv_values(
        &[
            "created",
            "updated",
            "deleted",
            "submitted",
            "queued",
            "started",
            "succeeded",
            "failed",
            "cancelled",
            "enabled",
            "disabled",
            "retry",
            "dead",
        ],
        prefix,
    )
}

pub fn audit_resources(_ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    complete_values(
        &[
            "namespace",
            "class",
            "object",
            "user",
            "group",
            "template",
            "remote-target",
        ],
        prefix,
    )
}

fn complete_values(values: &[&str], prefix: &str) -> Vec<String> {
    values
        .iter()
        .copied()
        .filter(|value| value.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

fn complete_csv_values(values: &[&str], prefix: &str) -> Vec<String> {
    let (head, tail) = prefix
        .rsplit_once(',')
        .map(|(head, tail)| (format!("{head},"), tail))
        .unwrap_or_else(|| (String::new(), prefix));

    values
        .iter()
        .copied()
        .filter(|value| value.starts_with(tail.trim_start()))
        .map(|value| format!("{head}{value}"))
        .collect()
}
