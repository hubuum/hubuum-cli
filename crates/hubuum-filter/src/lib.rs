use regex::Regex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Pipe error: {0}")]
    Pipe(String),

    #[error("Pipeline parse error: {0}")]
    Parse(String),

    #[error("Regular expression error: {0}")]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeStage {
    Grep(String),
    Reject(String),
    Head(usize),
    Tail(usize),
    Count,
    SortLines { descending: bool },
    Columns(Vec<String>),
    SortColumn { column: String, descending: bool },
}

impl PipeStage {
    pub fn apply_all(
        stages: &[Self],
        mut lines: Vec<String>,
    ) -> Result<Vec<String>, PipelineError> {
        for stage in stages {
            lines = stage.apply(lines)?;
        }
        Ok(lines)
    }

    fn apply(&self, lines: Vec<String>) -> Result<Vec<String>, PipelineError> {
        match self {
            Self::Grep(pattern) => {
                let regex = Regex::new(pattern)?;
                Ok(lines
                    .into_iter()
                    .filter(|line| regex.is_match(line))
                    .collect())
            }
            Self::Reject(pattern) => {
                let regex = Regex::new(pattern)?;
                Ok(lines
                    .into_iter()
                    .filter(|line| !regex.is_match(line))
                    .collect())
            }
            Self::Head(count) => Ok(lines.into_iter().take(*count).collect()),
            Self::Tail(count) => {
                let keep_from = lines.len().saturating_sub(*count);
                Ok(lines.into_iter().skip(keep_from).collect())
            }
            Self::Count => Ok(vec![lines.len().to_string()]),
            Self::SortLines { descending } => {
                let mut sorted = lines;
                sorted.sort();
                if *descending {
                    sorted.reverse();
                }
                Ok(sorted)
            }
            Self::Columns(_) | Self::SortColumn { .. } => Err(PipelineError::Pipe(
                "Pipe stage requires structured table output".to_string(),
            )),
        }
    }
}

pub fn split_pipeline(line: &str) -> Result<(String, Vec<PipeStage>), PipelineError> {
    let parts = split_unquoted_pipes(line);
    let Some(command) = parts.first() else {
        return Ok((String::new(), Vec::new()));
    };

    let stages = parts
        .iter()
        .skip(1)
        .map(|stage| parse_stage(stage.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok((command.trim().to_string(), stages))
}

fn split_unquoted_pipes(line: &str) -> Vec<String> {
    let mut escaped = false;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut start = 0;
    let mut parts = Vec::new();

    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !single_quoted => escaped = true,
            '\'' if !double_quoted => single_quoted = !single_quoted,
            '"' if !single_quoted => double_quoted = !double_quoted,
            '|' if !single_quoted && !double_quoted => {
                parts.push(line[start..index].to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(line[start..].to_string());
    parts
}

fn parse_stage(stage: &str) -> Result<PipeStage, PipelineError> {
    if stage.is_empty() {
        return Err(PipelineError::Pipe("Empty pipe stage".to_string()));
    }

    let Some(parts) = shlex::split(stage) else {
        return Err(PipelineError::Parse(
            "Parsing pipe stage failed".to_string(),
        ));
    };

    if parts.is_empty() {
        return Err(PipelineError::Pipe("Empty pipe stage".to_string()));
    }

    match parts[0].as_str() {
        "grep" | "F" => pattern_stage(parts[0].as_str(), &parts, PipeStage::Grep),
        "reject" => pattern_stage("reject", &parts, PipeStage::Reject),
        "head" | "L" => count_stage(parts[0].as_str(), &parts, PipeStage::Head),
        "tail" => count_stage("tail", &parts, PipeStage::Tail),
        "count" | "C" => {
            require_arg_count(parts[0].as_str(), &parts, 1)?;
            Ok(PipeStage::Count)
        }
        "columns" | "P" => parse_columns_stage(&parts),
        "sort" | "S" => parse_sort_stage(&parts),
        _ => parse_legacy_stage(stage),
    }
}

fn parse_legacy_stage(stage: &str) -> Result<PipeStage, PipelineError> {
    if let Some(pattern) = stage.strip_prefix('!') {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Err(PipelineError::Pipe(
                "Legacy reject filter requires a regex".to_string(),
            ));
        }
        Ok(PipeStage::Reject(pattern.to_string()))
    } else {
        Ok(PipeStage::Grep(stage.to_string()))
    }
}

fn pattern_stage(
    name: &str,
    parts: &[String],
    build: fn(String) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    require_arg_count(name, parts, 2)?;
    Ok(build(parts[1].clone()))
}

fn count_stage(
    name: &str,
    parts: &[String],
    build: fn(usize) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    if parts.len() > 2 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' accepts at most one count"
        )));
    }
    let count = parts
        .get(1)
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                PipelineError::Pipe(format!(
                    "Pipe stage '{name}' count must be a positive integer"
                ))
            })
        })
        .transpose()?
        .unwrap_or(10);
    Ok(build(count))
}

fn parse_columns_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            parts[0]
        )));
    }

    let columns = parts
        .iter()
        .skip(1)
        .flat_map(|part| part.split(','))
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if columns.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            parts[0]
        )));
    }

    Ok(PipeStage::Columns(columns))
}

fn parse_sort_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() > 3 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'sort' accepts: sort [line|column] [asc|desc]".to_string(),
        ));
    }

    let target = parts.get(1).map(String::as_str).unwrap_or("line");
    let (target, descending_prefix) = target
        .strip_prefix('!')
        .map(|target| (target, true))
        .unwrap_or((target, false));
    let descending = match parts.get(2).map(String::as_str).unwrap_or("asc") {
        "asc" => false,
        "desc" => true,
        other => {
            return Err(PipelineError::Pipe(format!(
                "Unknown sort direction '{other}'. Use asc or desc"
            )))
        }
    };

    let descending = descending || descending_prefix;

    if target == "line" {
        Ok(PipeStage::SortLines { descending })
    } else {
        Ok(PipeStage::SortColumn {
            column: target.to_string(),
            descending,
        })
    }
}

fn require_arg_count(name: &str, parts: &[String], expected: usize) -> Result<(), PipelineError> {
    if parts.len() != expected {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' expects {} argument(s)",
            expected.saturating_sub(1)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{split_pipeline, PipeStage};

    #[test]
    fn legacy_regex_pipe_still_parses() {
        let (command, stages) = split_pipeline("object list | alpha").expect("pipeline");
        assert_eq!(command, "object list");
        assert_eq!(stages, vec![PipeStage::Grep("alpha".to_string())]);
    }

    #[test]
    fn quoted_pipes_stay_in_command() {
        let (command, stages) =
            split_pipeline("object list --where name equals 'alpha|beta' | reject beta")
                .expect("pipeline");
        assert_eq!(command, "object list --where name equals 'alpha|beta'");
        assert_eq!(stages, vec![PipeStage::Reject("beta".to_string())]);
    }

    #[test]
    fn multiple_stages_apply_in_order() {
        let (_command, stages) =
            split_pipeline("object list | reject beta | sort line desc | head 2")
                .expect("pipeline");
        let lines = PipeStage::apply_all(
            &stages,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "delta".to_string(),
            ],
        )
        .expect("apply");
        assert_eq!(lines, vec!["gamma".to_string(), "delta".to_string()]);
    }

    #[test]
    fn count_replaces_lines_with_count() {
        let (_command, stages) = split_pipeline("object list | grep a | count").expect("pipeline");
        let lines = PipeStage::apply_all(&stages, vec!["alpha".to_string(), "beta".to_string()])
            .expect("apply");
        assert_eq!(lines, vec!["2".to_string()]);
    }

    #[test]
    fn head_and_tail_limit_lines() {
        let (_command, stages) = split_pipeline("object list | head 3 | tail 2").expect("pipeline");
        let lines = PipeStage::apply_all(
            &stages,
            vec![
                "one".to_string(),
                "two".to_string(),
                "three".to_string(),
                "four".to_string(),
            ],
        )
        .expect("apply");
        assert_eq!(lines, vec!["two".to_string(), "three".to_string()]);
    }

    #[test]
    fn structured_table_stages_parse_but_require_table_output() {
        let (_command, stages) =
            split_pipeline("object list | columns name,id | sort name desc").expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Columns(vec!["name".to_string(), "id".to_string()]),
                PipeStage::SortColumn {
                    column: "name".to_string(),
                    descending: true
                }
            ]
        );
        assert!(PipeStage::apply_all(&stages, vec!["plain text".to_string()]).is_err());
    }

    #[test]
    fn dsl_shorthand_aliases_parse() {
        let (_command, stages) =
            split_pipeline("object list | F active | P name id | S !name | L 5 | C")
                .expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Grep("active".to_string()),
                PipeStage::Columns(vec!["name".to_string(), "id".to_string()]),
                PipeStage::SortColumn {
                    column: "name".to_string(),
                    descending: true
                },
                PipeStage::Head(5),
                PipeStage::Count,
            ]
        );
    }

    #[test]
    fn projection_preserves_quoted_terms_with_spaces() {
        let (_command, stages) =
            split_pipeline("object list | P name 'team owner'").expect("pipeline");
        assert_eq!(
            stages,
            vec![PipeStage::Columns(vec![
                "name".to_string(),
                "team owner".to_string(),
            ])]
        );
    }
}
