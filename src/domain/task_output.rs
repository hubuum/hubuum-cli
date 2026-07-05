use serde::Serialize;

use crate::domain::{ImportResultRecord, ReportOutput};

#[derive(Debug, Clone, Serialize)]
pub enum TaskOutput {
    Report(ReportOutput),
    ImportResults(Vec<ImportResultRecord>),
    None,
}

impl TaskOutput {
    pub fn render_lines(&self) -> Vec<String> {
        match self {
            TaskOutput::None => Vec::new(),
            TaskOutput::Report(report) => match report {
                ReportOutput::Json { body } => {
                    vec![serde_json::to_string_pretty(body).unwrap_or_else(|_| "{}".to_string())]
                }
                ReportOutput::Rendered(rendered) => {
                    vec![rendered.body.clone()]
                }
            },
            TaskOutput::ImportResults(results) => results
                .iter()
                .map(|r| {
                    format!(
                        "{}: {} {} - {}",
                        r.0.entity_kind,
                        r.0.action,
                        r.0.identifier.as_deref().unwrap_or("<unknown>"),
                        r.0.outcome
                    )
                })
                .collect(),
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
