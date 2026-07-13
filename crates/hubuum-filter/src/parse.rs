use shlex::split;

use crate::error::PipelineError;
use crate::model::{AggregateFunction, AggregateSpec, GroupKey, PipeStage, ProjectTerm, SortCast};

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

    let Some(parts) = split(stage) else {
        return Err(PipelineError::Parse(
            "Parsing pipe stage failed".to_string(),
        ));
    };

    if parts.is_empty() {
        return Err(PipelineError::Pipe("Empty pipe stage".to_string()));
    }

    match parts[0].as_str() {
        "grep" | "F" => parse_filter_stage(parts[0].as_str(), &parts, PipeStage::Grep),
        "V" => parse_filter_stage("V", &parts, PipeStage::ValueSearch),
        "K" => parse_filter_stage("K", &parts, PipeStage::KeySearch),
        "?" => parse_truthy_stage(&parts),
        "reject" => parse_filter_stage("reject", &parts, PipeStage::Reject),
        "head" | "L" => parse_head_stage(parts[0].as_str(), &parts),
        "tail" => count_stage("tail", &parts, PipeStage::Tail),
        "count" | "C" => {
            require_arg_count(parts[0].as_str(), &parts, 1)?;
            Ok(PipeStage::Count)
        }
        "columns" | "P" => parse_columns_stage(&parts),
        "sort" | "S" => parse_sort_stage(&parts),
        "G" => parse_group_stage(&parts),
        "A" => parse_aggregate_stage(&parts),
        "Z" => {
            require_arg_count("Z", &parts, 1)?;
            Ok(PipeStage::CollapseGroups)
        }
        "U" => pattern_stage("U", &parts, PipeStage::Unroll),
        "JQ" => parse_jq_stage(&parts),
        "VALUE" | "VAL" => pattern_stage(parts[0].as_str(), &parts, PipeStage::Value),
        _ => parse_legacy_stage(stage, &parts),
    }
}

fn parse_filter_stage(
    name: &str,
    parts: &[String],
    build: fn(String) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' expects at least one argument"
        )));
    }

    if parts.len() == 2 {
        return Ok(build(parts[1].clone()));
    }

    Ok(build(format!(
        "{} contains {}",
        parts[1],
        parts[2..].join(" ")
    )))
}

fn parse_truthy_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() > 2 {
        return Err(PipelineError::Pipe(
            "Pipe stage '?' accepts at most one selector".to_string(),
        ));
    }
    Ok(PipeStage::Truthy(parts.get(1).cloned()))
}

fn parse_legacy_stage(stage: &str, parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts[0].len() == 1 && parts[0].chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(PipelineError::Parse(format!(
            "Unknown pipe stage '{}'",
            parts[0]
        )));
    }

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
    let count = parse_count(name, parts.get(1))?.unwrap_or(10);
    Ok(build(count))
}

fn parse_head_stage(name: &str, parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() > 3 {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{name}' accepts: {name} [count] [offset]"
        )));
    }
    Ok(PipeStage::Head {
        count: parse_count(name, parts.get(1))?.unwrap_or(10),
        offset: parse_count(name, parts.get(2))?.unwrap_or(0),
    })
}

fn parse_count(name: &str, value: Option<&String>) -> Result<Option<usize>, PipelineError> {
    value
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                PipelineError::Pipe(format!(
                    "Pipe stage '{name}' count must be a positive integer"
                ))
            })
        })
        .transpose()
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
        .map(|column| {
            column
                .strip_prefix('!')
                .map(ProjectTerm::drop)
                .unwrap_or_else(|| ProjectTerm::keep(column))
        })
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
    let mut position = 1;
    let target = parts.get(position).map(String::as_str).unwrap_or("line");
    if parts.get(position).is_some() {
        position += 1;
    }
    let (target, descending_prefix) = target
        .strip_prefix('!')
        .map(|target| (target, true))
        .unwrap_or((target, false));

    let mut descending = descending_prefix;
    if let Some(direction) = parts.get(position).map(String::as_str) {
        match direction {
            "asc" => {
                descending = descending_prefix;
                position += 1;
            }
            "desc" => {
                descending = true;
                position += 1;
            }
            _ => {}
        }
    }

    let cast = if parts.get(position).map(String::as_str) == Some("AS") {
        let Some(cast) = parts.get(position + 1) else {
            return Err(PipelineError::Pipe(
                "Sort cast requires AS num, AS str, or AS ip".to_string(),
            ));
        };
        position += 2;
        parse_sort_cast(cast)?
    } else {
        SortCast::Auto
    };

    if position < parts.len() {
        return Err(PipelineError::Pipe(
            "Pipe stage 'sort' accepts: sort [line|field] [asc|desc] [AS num|str|ip]".to_string(),
        ));
    }

    if target == "line" {
        Ok(PipeStage::SortLines { descending })
    } else {
        Ok(PipeStage::SortColumn {
            column: target.to_string(),
            descending,
            cast,
        })
    }
}

fn parse_sort_cast(value: &str) -> Result<SortCast, PipelineError> {
    match value {
        "num" | "number" => Ok(SortCast::Number),
        "str" | "string" => Ok(SortCast::String),
        "ip" => Ok(SortCast::Ip),
        other => Err(PipelineError::Pipe(format!(
            "Unknown sort cast '{other}'. Use num, str, or ip"
        ))),
    }
}

fn parse_group_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'G' requires at least one selector".to_string(),
        ));
    }

    let mut keys = Vec::new();
    let mut position = 1;
    while position < parts.len() {
        let selector = parts[position].clone();
        position += 1;
        let alias = if parts.get(position).map(String::as_str) == Some("AS") {
            let Some(alias) = parts.get(position + 1) else {
                return Err(PipelineError::Pipe(
                    "Group alias requires AS <name>".to_string(),
                ));
            };
            position += 2;
            alias.clone()
        } else {
            selector.clone()
        };
        keys.push(GroupKey { selector, alias });
    }

    Ok(PipeStage::Group(keys))
}

fn parse_aggregate_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'A' requires an aggregate expression".to_string(),
        ));
    }
    if parts.len() != 2 && parts.len() != 4 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'A' accepts: A <aggregate> [AS alias]".to_string(),
        ));
    }
    if parts.len() == 4 && parts[2] != "AS" {
        return Err(PipelineError::Pipe(
            "Aggregate alias requires AS <name>".to_string(),
        ));
    }

    let function = parse_aggregate_function(&parts[1])?;
    let alias = parts
        .get(3)
        .cloned()
        .unwrap_or_else(|| default_aggregate_alias(&function));
    Ok(PipeStage::Aggregate(AggregateSpec { function, alias }))
}

fn parse_aggregate_function(value: &str) -> Result<AggregateFunction, PipelineError> {
    if value == "count" {
        return Ok(AggregateFunction::Count);
    }

    let Some((name, rest)) = value.split_once('(') else {
        return Err(PipelineError::Pipe(format!(
            "Unknown aggregate '{value}'. Use count, sum(field), avg(field), min(field), or max(field)"
        )));
    };
    let Some(field) = rest.strip_suffix(')') else {
        return Err(PipelineError::Pipe(format!(
            "Malformed aggregate '{value}'"
        )));
    };
    if field.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Aggregate '{name}' requires a field"
        )));
    }

    match name {
        "sum" => Ok(AggregateFunction::Sum(field.to_string())),
        "avg" => Ok(AggregateFunction::Avg(field.to_string())),
        "min" => Ok(AggregateFunction::Min(field.to_string())),
        "max" => Ok(AggregateFunction::Max(field.to_string())),
        other => Err(PipelineError::Pipe(format!(
            "Unknown aggregate function '{other}'"
        ))),
    }
}

fn default_aggregate_alias(function: &AggregateFunction) -> String {
    match function {
        AggregateFunction::Count => "count".to_string(),
        AggregateFunction::Sum(field) => format!("sum({field})"),
        AggregateFunction::Avg(field) => format!("avg({field})"),
        AggregateFunction::Min(field) => format!("min({field})"),
        AggregateFunction::Max(field) => format!("max({field})"),
    }
}

fn parse_jq_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() < 2 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'JQ' requires an expression".to_string(),
        ));
    }
    Ok(PipeStage::Jq(parts[1..].join(" ")))
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
    use super::split_pipeline;
    use crate::model::{AggregateFunction, PipeStage, ProjectTerm, SortCast};

    #[test]
    fn dsl_shorthand_aliases_parse() {
        let (_command, stages) =
            split_pipeline("object list | F active | P name id | S !name | L 5 | C")
                .expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Grep("active".to_string()),
                PipeStage::Columns(vec![ProjectTerm::keep("name"), ProjectTerm::keep("id")]),
                PipeStage::SortColumn {
                    column: "name".to_string(),
                    descending: true,
                    cast: SortCast::Auto,
                },
                PipeStage::Head {
                    count: 5,
                    offset: 0
                },
                PipeStage::Count,
            ]
        );
    }

    #[test]
    fn grouping_and_aggregate_parse() {
        let (_command, stages) = split_pipeline(
            "object list --class Hosts | G os_version AS 'OS Version' | A sum(data.cpu.cores) AS Cores",
        )
        .expect("pipeline");

        assert!(matches!(stages[0], PipeStage::Group(_)));
        assert!(matches!(
            &stages[1],
            PipeStage::Aggregate(spec)
                if spec.alias == "Cores"
                    && spec.function == AggregateFunction::Sum("data.cpu.cores".to_string())
        ));
    }

    #[test]
    fn unknown_single_letter_stages_fail() {
        assert!(split_pipeline("object list | X thing").is_err());
        assert!(split_pipeline("object list | unknown thing").is_ok());
    }
}
