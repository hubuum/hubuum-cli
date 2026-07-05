use crate::commands::desired_format;
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

pub fn parse_task_submit_options(tokens: &CommandTokenizer) -> Result<TaskSubmitOptions, AppError> {
    let opts = tokens.get_options();

    let wait = opts.contains_key("wait");

    let timeout_secs = if let Some(timeout_str) = opts.get("timeout") {
        Some(timeout_str.parse::<u64>()?)
    } else {
        None
    };

    let poll_interval_secs = if let Some(poll_str) = opts.get("poll-interval") {
        Some(poll_str.parse::<u64>()?)
    } else {
        None
    };

    Ok(TaskSubmitOptions {
        wait,
        timeout_secs,
        poll_interval_secs,
    })
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
    use crate::commands::CliOption;
    use std::any::TypeId;

    fn opt(name: &str, short: Option<&str>, long: Option<&str>, flag: bool) -> CliOption {
        CliOption {
            name: name.to_string(),
            short: short.map(|s| s.to_string()),
            long: long.map(|l| l.to_string()),
            flag,
            greedy: false,
            nargs: None,
            repeatable: false,
            help: String::new(),
            field_type: TypeId::of::<String>(),
            field_type_help: "string".to_string(),
            required: false,
            autocomplete: None,
        }
    }

    #[test]
    fn options_default_is_background() {
        let o = TaskSubmitOptions::default();
        assert!(!o.wait);
    }

    #[test]
    fn parses_wait_flag() {
        let options = vec![opt("wait", None, Some("--wait"), true)];
        let tokens = CommandTokenizer::new("task submit --wait", "submit", &options)
            .expect("tokenization should succeed");

        let opts = parse_task_submit_options(&tokens).expect("parsing should succeed");
        assert!(opts.wait);
    }

    #[test]
    fn parses_wait_absent() {
        let options = vec![opt("wait", None, Some("--wait"), true)];
        let tokens = CommandTokenizer::new("task submit", "submit", &options)
            .expect("tokenization should succeed");

        let opts = parse_task_submit_options(&tokens).expect("parsing should succeed");
        assert!(!opts.wait);
    }

    #[test]
    fn parses_timeout() {
        let options = vec![opt("timeout", None, Some("--timeout"), false)];
        let tokens = CommandTokenizer::new("task submit --timeout 30", "submit", &options)
            .expect("tokenization should succeed");

        let opts = parse_task_submit_options(&tokens).expect("parsing should succeed");
        assert_eq!(opts.timeout_secs, Some(30));
    }

    #[test]
    fn parses_poll_interval() {
        let options = vec![opt("poll-interval", None, Some("--poll-interval"), false)];
        let tokens = CommandTokenizer::new("task submit --poll-interval 5", "submit", &options)
            .expect("tokenization should succeed");

        let opts = parse_task_submit_options(&tokens).expect("parsing should succeed");
        assert_eq!(opts.poll_interval_secs, Some(5));
    }

    #[test]
    fn parses_all_options() {
        let options = vec![
            opt("wait", None, Some("--wait"), true),
            opt("timeout", None, Some("--timeout"), false),
            opt("poll-interval", None, Some("--poll-interval"), false),
        ];
        let tokens = CommandTokenizer::new(
            "task submit --wait --timeout 30 --poll-interval 5",
            "submit",
            &options,
        )
        .expect("tokenization should succeed");

        let opts = parse_task_submit_options(&tokens).expect("parsing should succeed");
        assert!(opts.wait);
        assert_eq!(opts.timeout_secs, Some(30));
        assert_eq!(opts.poll_interval_secs, Some(5));
    }

    #[test]
    fn timeout_parse_error_returns_error() {
        let options = vec![opt("timeout", None, Some("--timeout"), false)];
        let tokens = CommandTokenizer::new("task submit --timeout invalid", "submit", &options)
            .expect("tokenization should succeed");

        let result = parse_task_submit_options(&tokens);
        assert!(result.is_err());
    }
}
