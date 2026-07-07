use crate::services::CompletionContext;

pub fn event_sinks(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.event_sinks(prefix)
}

pub fn users(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.users(prefix)
}

pub fn service_accounts(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.service_accounts(prefix)
}

pub fn remote_targets(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.remote_targets(prefix)
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

pub fn audit_event_ids(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.audit_event_ids(prefix)
}

pub fn event_delivery_ids(ctx: &CompletionContext, prefix: &str, _parts: &[String]) -> Vec<String> {
    ctx.event_delivery_ids(prefix)
}

pub fn audit_resource_names(
    ctx: &CompletionContext,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    match option_value(parts, "--resource").as_deref() {
        Some("namespace") => ctx.namespaces(prefix),
        Some("class") => ctx.classes(prefix),
        Some("object") => ctx.objects_from_class(prefix, parts, "--class"),
        Some("user") => ctx.users(prefix),
        Some("group") => ctx.groups(prefix),
        Some("template") => ctx.report_templates(prefix),
        Some("remote-target") => ctx.remote_targets(prefix),
        _ => Vec::new(),
    }
}

pub fn principal_names(ctx: &CompletionContext, prefix: &str, parts: &[String]) -> Vec<String> {
    match option_value(parts, "--principal-kind").as_deref() {
        Some("user") => ctx.users(prefix),
        Some("group") => ctx.groups(prefix),
        Some("service-account") => ctx.service_accounts(prefix),
        _ => Vec::new(),
    }
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

fn option_value(parts: &[String], long: &str) -> Option<String> {
    parts.iter().enumerate().find_map(|(index, part)| {
        if part == long {
            parts.get(index + 1).cloned()
        } else {
            part.strip_prefix(&format!("{long}=")).map(str::to_string)
        }
    })
}
