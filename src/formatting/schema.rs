use hubuum_client::{SchemaRevisionResponse, SchemaWorkResponse};
use serde_json::Value;

use crate::domain::SchemaOutput;

const MAX_FAILURE_GROUPS: usize = 5;
const MAX_LABEL_CHARS: usize = 100;

impl SchemaOutput {
    pub(crate) fn summary_lines(&self) -> Vec<String> {
        let mut lines = match self {
            Self::State(state) => {
                let mut lines = policy_lines(&state.active);
                lines.push(format!(
                    "Objects: {} valid, {} invalid, {} pending, {} not requiring validation",
                    state.counts.valid,
                    state.counts.invalid,
                    state.counts.pending,
                    state.counts.not_required
                ));
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
                    lines.push(format!("Resume revisions with --after {}", last.revision));
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
                    lines.push(format!("Next page: --after {after}"));
                }
                lines
            }
            Self::Activation(activation) => {
                let mut lines = policy_lines(&activation.active);
                if let Some(task) = activation.task_id {
                    lines.push(format!("Revalidation task: {task}"));
                }
                if let Some(task) = activation.dependent_rebuild_task_id {
                    lines.push(format!("Dependent rebuild task: {task}"));
                }
                lines
            }
            Self::Work(work) => work_lines(work),
        };
        lines.push("Full details: --output json".into());
        lines
    }
}

fn policy_lines(policy: &SchemaRevisionResponse) -> Vec<String> {
    vec![
        format!("Schema revision: {} ({})", policy.revision, policy.status),
        format!("Validation: {}", validation_label(policy.validate_schema)),
        format!("Schema: {}", schema_label(policy.json_schema.as_ref())),
    ]
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
    let readiness = work
        .readiness
        .map_or_else(|| "not available".into(), |value| value.to_string());
    let mut lines = vec![
        format!("Task: {} ({}; {})", work.task_id, work.kind, work.status),
        format!("Target schema revision: {}", work.target.revision),
        format!("Readiness: {readiness}"),
        format!(
            "Examined: {} objects in {} batches ({} ms)",
            work.examined, work.batches, work.elapsed_millis
        ),
        format!(
            "Results: {} valid, {} invalid, {} not requiring validation",
            work.valid, work.invalid, work.not_required
        ),
        format!(
            "Uninspectable: {}; stale: {}",
            work.uninspectable, work.stale
        ),
    ];
    if let Some(active) = &work.current_active_schema {
        lines.push(format!("Current active revision: {}", active.revision));
    }
    if let Some(impact) = &work.impact {
        lines.push(format!(
            "Compared with revision: {}",
            impact.baseline.revision
        ));
        let counts = &impact.counts;
        lines.push(format!(
            "Impact: {} newly invalid, {} still invalid, {} newly valid, {} still valid",
            counts.newly_invalid, counts.still_invalid, counts.newly_valid, counts.still_valid
        ));
        lines.push(format!("Validation: {} newly required and valid, {} no longer required, {} unchanged not required",
            counts.newly_required_valid, counts.no_longer_required, counts.unchanged_not_required));
        lines.push(format!(
            "Impact uninspectable: {}; ungrouped failures: {}",
            counts.uninspectable, impact.ungrouped_failures
        ));
        if !impact.failures.is_empty() {
            lines.push("Failure groups:".into());
            for group in impact.failures.iter().take(MAX_FAILURE_GROUPS) {
                let mut reason = compact_label(&group.reason.keyword);
                if let Some(property) = &group.reason.missing_property {
                    reason.push_str(&format!(" (missing {})", compact_label(property)));
                }
                if let Some(path) = &group.reason.schema_path {
                    reason.push_str(&format!(" at {}", compact_label(path)));
                }
                let samples = group
                    .samples
                    .iter()
                    .take(3)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "  {} objects: {reason}; sample IDs: {samples}",
                    group.objects
                ));
            }
            if impact.failures.len() > MAX_FAILURE_GROUPS {
                lines.push(format!(
                    "  {} more groups; use --output json for all groups.",
                    impact.failures.len() - MAX_FAILURE_GROUPS
                ));
            }
        }
        lines.push(format!(
            "Retained findings: {} (use --output json or generate-report for diagnostics)",
            impact.findings.len()
        ));
    }
    if !work.status.is_terminal() {
        lines.push(format!(
            "Poll: class schema work <class> --task {}",
            work.task_id
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
        assert!(summary.len() < 2500);
        assert!(summary.contains("Readiness: incompatible"));
        assert!(summary.contains("20 newly invalid"));
        assert!(summary.contains("Uninspectable: 2; stale: 1"));
        assert!(summary.contains("95 more groups"));
        assert!(summary.contains("Retained findings: 1000"));
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
        assert!(summary.contains("Schema: Device policy"));
        assert!(summary.contains("Validation: enabled"));
        assert!(!summary.contains("huge schema"));
        assert!(summary.contains("--output json"));
    }

    #[test]
    fn empty_compliance_pages_keep_the_continuation() {
        let page: SchemaCompliancePage =
            from_value(json!({"items": [], "next_after": 42})).unwrap();
        let summary = SchemaOutput::Compliance(page).summary_lines().join("\n");
        assert!(summary.contains("No visible objects"));
        assert!(summary.contains("Next page: --after 42"));
    }
}
