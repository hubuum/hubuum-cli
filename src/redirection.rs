use std::path::PathBuf;

use crate::errors::AppError;
use crate::output::OutputSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRedirect {
    pub path: PathBuf,
    pub append: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedirectCandidate {
    pub line: String,
    pub redirect: OutputRedirect,
}

pub(crate) fn split_redirect_candidate(line: &str) -> Result<Option<RedirectCandidate>, AppError> {
    let Some((operator_start, operator_len)) = final_redirect_operator(line) else {
        return Ok(None);
    };

    let command = line[..operator_start].trim_end();
    let target = line[operator_start + operator_len..].trim();
    if command.is_empty() {
        return Ok(None);
    }
    if target.is_empty() {
        return Err(AppError::ParseError(
            "Redirect requires a file path".to_string(),
        ));
    }

    let target_parts = shlex::split(target)
        .ok_or_else(|| AppError::ParseError("Parsing redirect path failed".to_string()))?;
    if target_parts.len() != 1 {
        return Err(AppError::ParseError(
            "Redirect accepts exactly one file path".to_string(),
        ));
    }

    Ok(Some(RedirectCandidate {
        line: command.to_string(),
        redirect: OutputRedirect {
            path: expand_user_path(&target_parts[0]),
            append: operator_len == 2,
        },
    }))
}

pub(crate) fn redirect_completion_context(line: &str, pos: usize) -> Option<(&str, usize)> {
    let prefix = line.get(..pos)?;
    let (operator_start, operator_len) = final_redirect_operator(prefix)?;
    let command = prefix[..operator_start].trim_end();
    if command.is_empty() {
        return None;
    }

    let target_start = operator_start + operator_len;
    let target = &prefix[target_start..];
    let leading_whitespace = target.len() - target.trim_start().len();
    let replacement_start = target_start + leading_whitespace;
    Some((&prefix[replacement_start..], replacement_start))
}

pub fn write_output(snapshot: &OutputSnapshot, redirect: &OutputRedirect) -> Result<(), AppError> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    if redirect.append {
        options.append(true);
    } else {
        options.truncate(true);
    }

    std::io::Write::write_all(
        &mut options.open(&redirect.path)?,
        snapshot.render().as_bytes(),
    )?;
    Ok(())
}

fn final_redirect_operator(line: &str) -> Option<(usize, usize)> {
    let mut quote = None;
    let mut escaped = false;
    let mut candidate = None;
    let mut iter = line.char_indices().peekable();

    while let Some((index, ch)) = iter.next() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if quote != Some('\'') => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '>' if quote.is_none() => {
                if iter.peek().is_some_and(|(_, next)| *next == '=') {
                    continue;
                }
                if iter.peek().is_some_and(|(_, next)| *next == '>') {
                    iter.next();
                    candidate = Some((index, 2));
                } else {
                    candidate = Some((index, 1));
                }
            }
            _ => {}
        }
    }

    candidate
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::{redirect_completion_context, split_redirect_candidate};

    #[test]
    fn splits_trailing_redirects() {
        let candidate = split_redirect_candidate("object list | P Name > out.json")
            .expect("redirect should parse")
            .expect("redirect should exist");

        assert_eq!(candidate.line, "object list | P Name");
        assert_eq!(
            candidate.redirect.path,
            std::path::PathBuf::from("out.json")
        );
        assert!(!candidate.redirect.append);
    }

    #[test]
    fn splits_append_redirects() {
        let candidate = split_redirect_candidate("object list >> out.json")
            .expect("redirect should parse")
            .expect("redirect should exist");

        assert_eq!(candidate.line, "object list");
        assert!(candidate.redirect.append);
    }

    #[test]
    fn ignores_quoted_redirects() {
        assert!(
            split_redirect_candidate("object list --where name equals 'a > b'")
                .expect("redirect parse should succeed")
                .is_none()
        );
    }

    #[test]
    fn completes_redirect_target() {
        assert_eq!(
            redirect_completion_context("object list > ou", "object list > ou".len()),
            Some(("ou", "object list > ".len()))
        );
    }
}
