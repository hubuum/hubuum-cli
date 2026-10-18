use hubuum_client::{
    QueryUsageAnalysisStatus, QueryUsagePattern, QueryUsageResourceOwnership, QueryUsageResponse,
};

use crate::domain::QueryUsageOutput;

impl QueryUsageOutput {
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match self {
            Self::List(declarations) => {
                for declaration in declarations {
                    lines.push(declaration_line(declaration));
                }
                if declarations.is_empty() {
                    lines.push("No query-usage declarations.".into());
                }
            }
            Self::Declaration(declaration) => lines.push(declaration_line(declaration)),
            Self::Deleted { deleted } => {
                lines.push(format!("Deleted query-usage declaration {deleted}."))
            }
            Self::Analysis(report) => {
                let status = match report.status {
                    QueryUsageAnalysisStatus::Unavailable => "unavailable",
                    QueryUsageAnalysisStatus::InsufficientEvidence => "insufficient evidence",
                    QueryUsageAnalysisStatus::Complete => "complete",
                    _ => "unknown",
                };
                lines.push(format!("Analysis: {status}"));
                lines.push(format!("Assessed: {}", report.assessed_at));
                lines.push(format!(
                    "Observations: {} (sample every {}; {} patterns; {} capacity drops)",
                    if report.observations.settings.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    report.observations.settings.sample_every,
                    report.observations.patterns.len(),
                    report.observations.capacity_drops
                ));
                for observation in &report.observations.patterns {
                    lines.push(format!(
                        "  {}: {} sampled queries; {} microseconds across whole queries",
                        pattern_line(&observation.pattern),
                        observation.sampled_queries,
                        observation.whole_query_duration_micros
                    ));
                }
                for assessment in &report.assessments {
                    lines.push(format!(
                        "Assessment {}: schema {:?}; can prepare: {}",
                        pattern_line(&assessment.pattern),
                        assessment.schema_compatibility,
                        assessment.can_prepare
                    ));
                    lines.push(format!("  {}", assessment.rationale));
                    for resource in &assessment.resources {
                        let ownership = match resource.ownership {
                            QueryUsageResourceOwnership::DeclarationManaged => {
                                "declaration managed"
                            }
                            QueryUsageResourceOwnership::IndependentlyManaged => {
                                "independently managed"
                            }
                            _ => "unknown ownership",
                        };
                        lines.push(format!("  Resource {}: {ownership}, {} bytes, scans {}, owners {}, cleanup eligible if removed: {}",
                            resource.reference, resource.bytes,
                            resource.native_scan_count.map_or_else(|| "unknown".into(), |value| value.to_string()),
                            resource.declaration_owners.map_or_else(|| "unknown".into(), |value| value.to_string()),
                            resource.cleanup_eligible_if_removed));
                    }
                    if let Some(progress) = &assessment.adapter_progress {
                        lines.push(format!(
                            "  Adapter {}: {} ({} owners; updated {})",
                            progress.reference,
                            progress.state,
                            progress.declaration_owners,
                            progress.updated_at
                        ));
                        if let Some(error) = &progress.last_error {
                            lines.push(format!("  Last error: {error}"));
                        }
                    }
                }
                lines.push(format!("Suggestions: {}", report.suggestions.len()));
                for suggestion in &report.suggestions {
                    lines.push(format!(
                        "  {}: {}",
                        pattern_line(&suggestion.proposed),
                        suggestion.rationale
                    ));
                }
                for limitation in &report.limitations {
                    lines.push(format!("Limitation: {limitation}"));
                }
                lines.push("Analysis does not adopt suggestions or prepare indexes. Missing observations or zero scans do not prove a resource is unused.".into());
            }
        }
        if !matches!(self, Self::Deleted { .. }) {
            lines.push("Full details: --output json".into());
        }
        lines
    }
}

fn pattern_line(pattern: &QueryUsagePattern) -> String {
    format!(
        "{} ({:?}; {})",
        pattern.path(),
        pattern.value_type(),
        pattern
            .operations()
            .iter()
            .map(|operation| format!("{operation:?}").to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn declaration_line(declaration: &QueryUsageResponse) -> String {
    format!(
        "{}  revision {}  {}  schema {:?}",
        declaration.id,
        declaration.revision,
        pattern_line(&declaration.pattern),
        declaration.schema_compatibility
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn analysis_distinguishes_statuses_and_preserves_complete_empty_suggestions() {
        for (status, expected) in [
            ("unavailable", "unavailable"),
            ("insufficient_evidence", "insufficient evidence"),
            ("complete", "complete"),
        ] {
            let report = from_value(json!({
                "status": status, "assessed_at": "2026-10-19T00:00:00Z",
                "observations": {"source":"process", "settings":{"enabled":false,"sample_every":16,"max_patterns":2048,"max_patterns_per_class":64,"retention_seconds":86400,"max_predicates_per_query":16},
                    "process_started_at":"2026-10-19T00:00:00Z", "captured_at":"2026-10-19T00:00:00Z", "patterns":[], "capacity_drops":0},
                "assessments":[], "suggestions":[], "limitations":["no measurements"]
            })).unwrap();
            let lines = QueryUsageOutput::Analysis(Box::new(report)).summary_lines();
            assert_eq!(lines[0], format!("Analysis: {expected}"));
            assert!(lines.iter().any(|line| line == "Suggestions: 0"));
            assert!(lines
                .iter()
                .any(|line| line.contains("Observations: disabled")));
            assert!(lines.iter().any(|line| line.contains("no measurements")));
        }
    }
    #[test]
    fn native_resources_keep_unknown_scans_ownership_and_adapter_failures_visible() {
        let pattern = json!({"path":"serial","value_type":"string","operations":["equals"]});
        let report = from_value(json!({
            "status":"complete", "assessed_at":"2026-10-19T00:00:00Z",
            "observations":{"source":"process","settings":{"enabled":true,"sample_every":16,"max_patterns":2048,"max_patterns_per_class":64,"retention_seconds":86400,"max_predicates_per_query":16},
                "process_started_at":"2026-10-19T00:00:00Z","captured_at":"2026-10-19T00:00:00Z","patterns":[],"capacity_drops":3},
            "assessments":[{"declaration_id":7,"pattern":pattern,"schema_compatibility":"compatible","can_prepare":true,
                "resources":[{"reference":"shared_idx","ownership":"declaration_managed","bytes":8192,"native_scan_count":null,"declaration_owners":2,"cleanup_eligible_if_removed":false},
                    {"reference":"external_idx","ownership":"independently_managed","bytes":4096,"native_scan_count":0,"declaration_owners":null,"cleanup_eligible_if_removed":false}],
                "adapter_progress":{"reference":"shared_idx","state":"failed","declaration_owners":2,"last_error":"permission denied","updated_at":"2026-10-19T00:00:00Z"},
                "rationale":"shared ownership"}],
            "suggestions":[],"limitations":[]
        })).unwrap();
        let output = QueryUsageOutput::Analysis(Box::new(report));
        let text = output.summary_lines().join("\n");
        assert!(
            text.contains("shared_idx: declaration managed, 8192 bytes, scans unknown, owners 2")
        );
        assert!(text
            .contains("external_idx: independently managed, 4096 bytes, scans 0, owners unknown"));
        assert!(text.contains("Adapter shared_idx: failed"));
        assert!(text.contains("Last error: permission denied"));
        assert!(text.contains("3 capacity drops"));
        let json = serde_json::to_value(output).unwrap();
        assert!(json["assessments"][0]["resources"][0]["native_scan_count"].is_null());
        assert_eq!(
            json["assessments"][0]["adapter_progress"]["last_error"],
            "permission denied"
        );
    }
}
