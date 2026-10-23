use serde::Serialize;
use serde_json::to_value;

use crate::command_line::rebuild_with_replaced_options;
use crate::commands::render_format;
use crate::config::get_config;
use crate::errors::AppError;
use crate::formatting::{OutputFormatter, TableRenderable};
use crate::models::OutputFormat;
use crate::output::{
    add_warning, append_line, append_pipeline_suffix, has_pipeline, pipeline_suppresses_pagination,
    set_next_page_command, set_semantic_output, RenderFormat,
};
use crate::tokenizer::CommandTokenizer;
use hubuum_filter::OutputEnvelope;

use super::{PagedResult, ServerPageSize, PARTIAL_PIPELINE_WARNING};

pub fn render_paged_result<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
    format: OutputFormat,
) -> Result<(), AppError>
where
    T: Serialize + Clone + TableRenderable,
{
    warn_on_partial_pipeline(paged)?;
    match format {
        OutputFormat::Json => {
            if has_pipeline()? {
                paged.items.format_noreturn()?;
            } else {
                set_paged_json_output(tokens, paged)?;
            }
        }
        OutputFormat::Text => {
            paged.items.format_noreturn()?;
            append_paging_footer(tokens, paged)?;
        }
    }
    Ok(())
}

pub(crate) fn warn_on_partial_pipeline<T>(paged: &PagedResult<T>) -> Result<(), AppError> {
    if paged.next_cursor.is_some() && has_pipeline()? {
        add_warning(PARTIAL_PIPELINE_WARNING)?;
    }
    Ok(())
}

pub(crate) fn set_paged_json_output<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
) -> Result<(), AppError>
where
    T: Serialize,
{
    if should_wrap_paged_json(tokens, paged) {
        set_semantic_output(OutputEnvelope::detail(to_value(paged)?, Vec::new()))?;
    } else {
        set_semantic_output(OutputEnvelope::rows(
            to_value(&paged.items)?
                .as_array()
                .cloned()
                .unwrap_or_default(),
            Vec::new(),
        ))?;
    }
    Ok(())
}

pub(crate) fn append_paging_footer<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
) -> Result<(), AppError> {
    if render_format(tokens)? != RenderFormat::Text {
        return Ok(());
    }

    if pipeline_suppresses_pagination()? {
        return Ok(());
    }

    if !tokens.get_options().contains_key("limit")
        && paged.next_cursor.is_none()
        && paged.total_count.is_none()
    {
        return Ok(());
    }

    let qualifiers = [paged.total_count.map(|total| format!("total: {total}"))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    append_line(format!(
        "Returned {} item(s){}",
        paged.returned_count,
        if qualifiers.is_empty() {
            String::new()
        } else {
            format!(" ({})", qualifiers.join(", "))
        }
    ))?;
    if should_offer_next_page(paged) {
        let next_cursor = paged.next_cursor.as_deref().expect("checked above");
        let next_command = next_cursor_command(tokens, next_cursor)?;
        set_next_page_command(next_command)?;
        if get_config().repl.enter_fetches_next_page {
            append_line(
                "Paginated results available. Press Enter for the next page, or Esc/Ctrl-C to stop.",
            )?;
        } else {
            append_line(
                "Paginated results available. Type 'next' for the next page, or Esc/Ctrl-C to stop.",
            )?;
        }
    }
    Ok(())
}

pub(super) fn should_wrap_paged_json<T>(tokens: &CommandTokenizer, paged: &PagedResult<T>) -> bool {
    paged.total_count.is_some()
        || tokens.get_options().contains_key("cursor")
        || paged.next_cursor.is_some()
}

fn should_offer_next_page<T>(paged: &PagedResult<T>) -> bool {
    paged.next_cursor.is_some()
}

pub(super) fn next_cursor_command(
    tokens: &CommandTokenizer,
    cursor: &str,
) -> Result<String, AppError> {
    let requested_page_size = tokens
        .get_options()
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .map(ServerPageSize::from_requested)
        .transpose()?;

    let command = match requested_page_size.filter(|page_size| page_size.was_truncated()) {
        Some(page_size) => {
            let effective = page_size.effective().to_string();
            rebuild_with_replaced_options(
                tokens,
                &["--limit", "--cursor"],
                [
                    ("--limit", Some(effective.as_str())),
                    ("--cursor", Some(cursor)),
                ],
            )
        }
        None => rebuild_with_replaced_options(tokens, &["--cursor"], [("--cursor", Some(cursor))]),
    };

    append_pipeline_suffix(command)
}
