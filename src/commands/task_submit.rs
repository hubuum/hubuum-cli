use crate::commands::{desired_format};
use crate::domain::TaskRecord;
use crate::errors::AppError;
use crate::formatting::OutputFormatter;
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, WaitTaskInput};
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Clone, Default)]
pub struct TaskSubmitOptions {
    pub wait: bool,
    pub timeout_secs: Option<u64>,
    pub poll_interval_secs: Option<u64>,
}

pub fn run_task_backed(
    services: &AppServices,
    tokens: &CommandTokenizer,
    label: impl Into<String>,
    opts: TaskSubmitOptions,
    task: TaskRecord,
) -> Result<(), AppError> {
    let label = label.into();
    let task_id = task.0.id;
    let kind = task.0.kind;

    if opts.wait {
        let final_task = services.gateway().wait_task(WaitTaskInput {
            task_id,
            timeout_secs: opts.timeout_secs,
            poll_interval_secs: opts.poll_interval_secs,
        })?;
        let output = services.gateway().task_output(task_id)?;
        match desired_format(tokens) {
            OutputFormat::Json => append_line(serde_json::to_string_pretty(&output)?)?,
            OutputFormat::Text => {
                final_task.format_noreturn()?;
                for line in output.render_lines() {
                    append_line(line)?;
                }
            }
        }
        return Ok(());
    }

    let registration = services.background().watch_task(task.clone(), label);
    match desired_format(tokens) {
        OutputFormat::Json => append_line(serde_json::to_string_pretty(&task)?)?,
        OutputFormat::Text => {
            append_line(format!("submitted task #{task_id} ({kind})"))?;
            if let Some(reg) = registration {
                append_line(format!("tracking as background job {}", reg.local_id))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_default_is_background() {
        let o = TaskSubmitOptions::default();
        assert!(!o.wait);
    }
}
