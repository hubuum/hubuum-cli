use hubuum_client::{SchemaRevisionResponse, SchemaWorkResponse};
use serde_json::Value;

use crate::config::get_config;
use crate::domain::SchemaOutput;
use crate::output::render_detail_field;

const MAX_FAILURE_GROUPS: usize = 5;
const MAX_LABEL_CHARS: usize = 100;

impl SchemaOutput {
    pub(crate) fn summary_lines(&self) -> Vec<String> {
        let mut lines = match self {
            Self::State(state) => {
                let mut lines = policy_lines(&state.active);
                section(
                    &mut lines,
                    "Objects",
                    vec![
                        ("Valid", state.counts.valid.to_string()),
                        ("Invalid", state.counts.invalid.to_string()),
                        ("Pending", state.counts.pending.to_string()),
                        ("Not required", state.counts.not_required.to_string()),
                    ],
                );
                lines
            }
            Self::Revision(policy) => policy_lines(policy),
            Self::Revisions(policies) => {
                let mut lines = vec!["Revision  Status       Validation  Schema".into()];
                for policy in policies {
                    lines.push(format!(
                        "{:<9} {:<12} {:<11} {}",
                        policy.revision,
                        policy.status,
                        validation_label(policy.validate_schema),
                        schema_label(policy.json_schema.as_ref())
                    ));
                }
                if let Some(last) = policies.last() {
                    lines.push(String::new());
                    lines.extend(detail_lines(
                        vec![("Next page", format!("--after {}", last.revision))],
                        0,
                    ));
                } else {
                    lines.push("No revisions returned.".into());
                }
                lines
            }
            Self::Compliance(page) => {
                let mut lines = vec!["Object  Revision  Compliance    Schema revision".into()];
                for object in &page.items {
                    lines.push(format!(
                        "{:<7} {:<9} {:<13} {}",
                        object.object_id,
                        object.object_revision,
                        object.status,
                        object.active_schema.revision
                    ));
                }
                if page.items.is_empty() {
                    lines.push("No visible objects on this page.".into());
                }
                if let Some(after) = page.next_after {
                    lines.push(String::new());
                    lines.extend(detail_lines(
                        vec![("Next page", format!("--after {after}"))],
                        0,
                    ));
                }
                lines
            }
            Self::Activation(activation) => {
                let mut lines = policy_lines(&activation.active);
                let mut tasks = Vec::new();
                if let Some(task) = activation.task_id {
                    tasks.push(("Revalidation", task.to_string()));
                }
                if let Some(task) = activation.dependent_rebuild_task_id {
                    tasks.push(("Dependent rebuild", task.to_string()));
                }
                if !tasks.is_empty() {
                    section(&mut lines, "Tasks", tasks);
                }
                lines
            }
            Self::Work(work) => work_lines(work),
        };
        lines.push(String::new());
        let details = if matches!(self, Self::Work(_)) {
            "--output json or generate-report"
        } else {
            "--output json"
        };
        lines.extend(detail_lines(vec![("Full details", details.into())], 0));
        lines
    }
}

fn detail_lines(rows: Vec<(&str, String)>, indent: usize) -> Vec<String> {
    let padding = rows
        .iter()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or_default()
        .max(usize::try_from(get_config().output.padding).unwrap_or_default())
        .saturating_add(1);
    let prefix = " ".repeat(indent);
    rows.into_iter()
        .map(|(key, value)| format!("{prefix}{}", render_detail_field(key, &value, padding)))
        .collect()
}

fn section(lines: &mut Vec<String>, title: &str, rows: Vec<(&str, String)>) {
    lines.push(String::new());
    lines.push(format!("{title}:"));
    lines.extend(detail_lines(rows, 2));
}

fn policy_lines(policy: &SchemaRevisionResponse) -> Vec<String> {
    detail_lines(
        vec![
            ("Schema revision", policy.revision.to_string()),
            ("Status", policy.status.to_string()),
            (
                "Validation",
                validation_label(policy.validate_schema).into(),
            ),
            ("Schema", schema_label(policy.json_schema.as_ref())),
        ],
        0,
    )
}

fn validation_label(enabled: bool) -> &'static str {
    if enabled {
        "enabled"
    } else {
        "disabled"
    }
}

fn schema_label(schema: Option<&Value>) -> String {
    match schema {
        None | Some(Value::Null) => "none".into(),
        Some(Value::Bool(value)) => format!("{value} (boolean schema)"),
        Some(schema) => {
            let name = schema
                .get("title")
                .or_else(|| schema.get("$id"))
                .and_then(Value::as_str);
            name.map_or_else(|| "present".into(), compact_label)
        }
    }
}

fn work_lines(work: &SchemaWorkResponse) -> Vec<String> {
    let mut header = vec![
        ("Task", work.task_id.to_string()),
        ("Kind", work.kind.to_string()),
        ("Status", work.status.to_string()),
        (
            "Readiness",
            work.readiness
                .map_or_else(|| "not available".into(), |value| value.to_string()),
        ),
        ("Schema revision", work.target.revision.to_string()),
    ];
    if let Some(active) = &work.current_active_schema {
        header.push(("Active revision", active.revision.to_string()));
    }
    if let Some(impact) = &work.impact {
        header.push(("Baseline revision", impact.baseline.revision.to_string()));
    }
    header.push((
        "Examined",
        format!(
            "{} objects in {} batches ({} ms)",
            work.examined, work.batches, work.elapsed_millis
        ),
    ));
    let mut lines = detail_lines(header, 0);
    let mut results = vec![
        ("Valid", work.valid.to_string()),
        ("Invalid", work.invalid.to_string()),
    ];
    for (label, count) in [
        ("Not required", work.not_required),
        ("Uninspectable", work.uninspectable),
        ("Stale", work.stale),
    ] {
        if count > 0 {
            results.push((label, count.to_string()));
        }
    }
    section(&mut lines, "Results", results);
    if let Some(impact) = &work.impact {
        let counts = &impact.counts;
        let mut rows = Vec::new();
        for (label, count) in [
            ("Newly invalid", counts.newly_invalid),
            ("Still invalid", counts.still_invalid),
            ("Newly valid", counts.newly_valid),
            ("Still valid", counts.still_valid),
            ("Newly required valid", counts.newly_required_valid),
            ("No longer required", counts.no_longer_required),
            ("Unchanged not required", counts.unchanged_not_required),
            ("Uninspectable", counts.uninspectable),
            ("Ungrouped failures", impact.ungrouped_failures),
        ] {
            if count > 0 {
                rows.push((label, count.to_string()));
            }
        }
        if rows.is_empty() {
            rows.push(("Changes", "0 reported".into()));
        }
        rows.push(("Retained findings", impact.findings.len().to_string()));
        section(&mut lines, "Impact", rows);
        if !impact.failures.is_empty() {
            lines.push(String::new());
            lines.push("Failures:".into());
            for (index, group) in impact.failures.iter().take(MAX_FAILURE_GROUPS).enumerate() {
                if index > 0 {
                    lines.push(String::new());
                }
                lines.push(format!(
                    "  {} objects: {}",
                    group.objects,
                    compact_label(&group.reason.keyword)
                ));
                let mut rows = Vec::new();
                if let Some(property) = &group.reason.missing_property {
                    rows.push(("Missing property", compact_label(property)));
                }
                if let Some(path) = &group.reason.schema_path {
                    rows.push(("Schema path", compact_label(path)));
                }
                if !group.samples.is_empty() {
                    rows.push((
                        "Sample IDs",
                        group
                            .samples
                            .iter()
                            .take(3)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                lines.extend(detail_lines(rows, 4));
            }
            if impact.failures.len() > MAX_FAILURE_GROUPS {
                lines.push(format!(
                    "  {} more groups; use --output json for all groups.",
                    impact.failures.len() - MAX_FAILURE_GROUPS
                ));
            }
        }
    }
    if !work.status.is_terminal() {
        lines.push(String::new());
        lines.extend(detail_lines(
            vec![(
                "Poll",
                format!("class schema work <class> --task {}", work.task_id),
            )],
            0,
        ));
    }
    lines
}

fn compact_label(value: &str) -> String {
    let mut chars = value.chars().map(|c| if c.is_control() { ' ' } else { c });
    let mut label: String = chars.by_ref().take(MAX_LABEL_CHARS).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    label
}

#[cfg(test)]
mod tests {
    use hubuum_client::{SchemaCompliancePage, SchemaRevisionResponse};
    use serde_json::{from_str, from_value, json, to_value};

    use super::*;

    #[test]
    fn impact_summary_is_bounded_and_keeps_readiness_and_failure_counts() {
        let mut work: SchemaWorkResponse =
            from_str(include_str!("../../tests/fixtures/schema-work.json")).unwrap();
        let impact = work.impact.as_mut().unwrap();
        let mut group = impact.failures[0].clone();
        group.reason.schema_path = Some("x".repeat(10000));
        impact.failures = vec![group; 100];
        impact.findings = vec![impact.findings[0].clone(); 1000];
        let output = SchemaOutput::Work(Box::new(work));
        let summary = output.summary_lines().join("\n");
        assert!(summary.len() < 3000);
        let fields: Vec<_> = summary
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(label, value)| (label.trim(), value.trim()))
            .collect();
        assert!(fields.contains(&("Readiness", "incompatible")));
        assert!(fields.contains(&("Newly invalid", "20")));
        assert!(fields.contains(&("Uninspectable", "2")));
        assert!(fields.contains(&("Stale", "1")));
        assert!(summary.contains("95 more groups"));
        assert!(fields.contains(&("Retained findings", "1000")));
        assert!(!summary.contains("\"snapshot\""));
        assert_eq!(
            to_value(output).unwrap()["impact"]["findings"]
                .as_array()
                .unwrap()
                .len(),
            1000
        );
    }

    #[test]
    fn policy_summary_never_dumps_the_schema_document() {
        let policy: SchemaRevisionResponse = from_value(json!({
            "class_id": 9, "revision": 2, "status": "staged", "validate_schema": true,
            "created_at": "2026-09-16T00:00:00Z", "json_schema": {
                "title": "Device policy", "description": "huge schema".repeat(10000)
            }
        }))
        .unwrap();
        let summary = SchemaOutput::Revision(policy).summary_lines().join("\n");
        assert!(summary
            .lines()
            .any(|line| line.starts_with("Schema ") && line.ends_with(": Device policy")));
        assert!(summary
            .lines()
            .any(|line| line.starts_with("Validation ") && line.ends_with(": enabled")));
        assert!(!summary.contains("huge schema"));
        assert!(summary.contains("--output json"));
    }

    #[test]
    fn empty_compliance_pages_keep_the_continuation() {
        let page: SchemaCompliancePage =
            from_value(json!({"items": [], "next_after": 42})).unwrap();
        let summary = SchemaOutput::Compliance(page).summary_lines().join("\n");
        assert!(summary.contains("No visible objects"));
        assert!(summary
            .lines()
            .any(|line| line.starts_with("Next page ") && line.ends_with(": --after 42")));
    }
}
