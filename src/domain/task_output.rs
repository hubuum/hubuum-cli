use serde::Serialize;

use crate::domain::{ImportResultRecord, RemoteCallRecord, ReportOutput};

#[derive(Debug, Clone, Serialize)]
pub enum TaskOutput {
    Report(ReportOutput),
    ImportResults(Vec<ImportResultRecord>),
    RemoteCall(RemoteCallRecord),
    None,
}

impl TaskOutput {
    pub fn render_lines(&self) -> Vec<String> {
        match self {
            TaskOutput::None => Vec::new(),
            TaskOutput::Report(report) => {
                match report {
                    ReportOutput::Json { body } => {
                        vec![serde_json::to_string_pretty(body).unwrap_or_else(|_| "{}".to_string())]
                    }
                    ReportOutput::Rendered(rendered) => {
                        vec![rendered.body.clone()]
                    }
                }
            }
            TaskOutput::ImportResults(results) => {
                results.iter().map(|r| {
                    format!("{}: {} {} - {}",
                        r.0.entity_kind,
                        r.0.action,
                        r.0.identifier.as_deref().unwrap_or("<unknown>"),
                        r.0.outcome)
                }).collect()
            }
            TaskOutput::RemoteCall(result) => {
                let mut lines = Vec::new();
                lines.push(format!("Remote Call ID: {}", result.0.id));
                lines.push(format!("Task ID: {}", result.0.task_id));
                lines.push(format!("Success: {}", result.0.success));
                lines.push(format!("Method: {}", result.0.method));
                lines.push(format!("URL: {}", result.0.rendered_url));
                if let Some(status) = result.0.response_status {
                    lines.push(format!("Response Status: {}", status));
                }
                if let Some(error) = &result.0.error {
                    lines.push(format!("Error: {}", error));
                }
                lines.push(format!("Duration: {}ms", result.0.duration_ms));
                lines
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_output_none_renders_empty() {
        let out = TaskOutput::None;
        assert!(out.render_lines().is_empty());
    }
}
