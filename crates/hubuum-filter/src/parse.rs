use std::collections::HashSet;

use shlex::split;

use crate::error::PipelineError;
use crate::model::{
    validate_group_keys, validate_projection_terms, AggregateFunction, AggregateSpec, GroupKey,
    NullOrder, PipeStage, ProjectTerm, SortCast, SortDirection, SortKey, SortReduction, SortSpec,
};
use crate::predicate::Predicate;
use crate::selector::Selector;
use crate::verbs::search::validate_filter_expression;

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
    validate_pipeline_output_names(&stages)?;

    Ok((command.trim().to_string(), stages))
}

pub(crate) fn parse_stage_list(source: &str) -> Result<Vec<PipeStage>, PipelineError> {
    if source.trim().is_empty() {
        return Ok(Vec::new());
    }

    let parts = split_unquoted_pipes(source);
    let start = usize::from(parts.first().is_some_and(|part| part.trim().is_empty()));
    let stages = parts
        .iter()
        .skip(start)
        .map(|stage| parse_stage(stage.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    if stages.is_empty() {
        return Err(PipelineError::Pipe(
            "Pipeline requires at least one stage after '|'".to_string(),
        ));
    }
    Ok(stages)
}

pub(crate) fn validate_pipeline_output_names(stages: &[PipeStage]) -> Result<(), PipelineError> {
    let mut grouped_names = None::<HashSet<String>>;
    for stage in stages {
        match stage {
            PipeStage::Group(keys) => {
                grouped_names = Some(keys.iter().map(|key| key.alias().to_string()).collect());
            }
            PipeStage::Aggregate(spec) => {
                if let Some(names) = &mut grouped_names {
                    if !names.insert(spec.alias().to_string()) {
                        return Err(PipelineError::Pipe(format!(
                            "Pipe stage 'A' output name '{}' conflicts with a group key or earlier aggregate",
                            spec.alias()
                        )));
                    }
                }
            }
            PipeStage::Columns(terms) if grouped_names.is_some() => {
                let keepers = terms
                    .iter()
                    .filter(|term| !term.is_drop())
                    .map(|term| term.selector().to_string())
                    .collect::<HashSet<_>>();
                if !keepers.is_empty() {
                    grouped_names = Some(keepers);
                } else if let Some(names) = &mut grouped_names {
                    for term in terms.iter().filter(|term| term.is_drop()) {
                        names.remove(term.selector().as_str());
                    }
                }
            }
            PipeStage::Count
            | PipeStage::CollapseGroups
            | PipeStage::Jq(_)
            | PipeStage::Value(_) => grouped_names = None,
            PipeStage::Grep(_)
            | PipeStage::TypedFilter(_)
            | PipeStage::ValueSearch(_)
            | PipeStage::KeySearch(_)
            | PipeStage::Truthy(_)
            | PipeStage::Reject(_)
            | PipeStage::TypedReject(_)
            | PipeStage::Head { .. }
            | PipeStage::Tail(_)
            | PipeStage::SortLines { .. }
            | PipeStage::Columns(_)
            | PipeStage::SortColumns(_)
            | PipeStage::Unroll(_) => {}
        }
    }
    Ok(())
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

    if let Some(typed) = parse_typed_predicate_stage(stage) {
        return typed;
    }
    if let Some(sort) = parse_sort_stage_source(stage) {
        return sort;
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
        "sort" | "S" => unreachable!("sort stages are parsed from their original source"),
        "G" => parse_group_stage(&parts),
        "A" => parse_aggregate_stage(&parts),
        "Z" => {
            require_arg_count("Z", &parts, 1)?;
            Ok(PipeStage::CollapseGroups)
        }
        "U" => selector_stage("U", &parts, PipeStage::Unroll),
        "JQ" => parse_jq_stage(&parts),
        "VALUE" | "VAL" => selector_stage(parts[0].as_str(), &parts, PipeStage::Value),
        _ => parse_legacy_stage(stage, &parts),
    }
}

fn parse_typed_predicate_stage(stage: &str) -> Option<Result<PipeStage, PipelineError>> {
    let verb_end = stage
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))?;
    let verb = &stage[..verb_end];
    if !matches!(verb, "F" | "grep" | "reject") {
        return None;
    }

    let rest = stage[verb_end..].trim_start();
    let where_end = rest
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(rest.len());
    if !rest[..where_end].eq_ignore_ascii_case("WHERE") {
        return None;
    }

    let source = rest[where_end..].trim_start();
    let parsed = Predicate::parse(source).map(|predicate| {
        if verb == "reject" {
            PipeStage::TypedReject(predicate)
        } else {
            PipeStage::TypedFilter(predicate)
        }
    });
    Some(parsed)
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

    let expression = if parts.len() == 2 {
        parts[1].clone()
    } else {
        format!("{} contains {}", parts[1], parts[2..].join(" "))
    };
    validate_filter_expression(&expression)?;
    Ok(build(expression))
}

fn parse_truthy_stage(parts: &[String]) -> Result<PipeStage, PipelineError> {
    if parts.len() > 2 {
        return Err(PipelineError::Pipe(
            "Pipe stage '?' accepts at most one selector".to_string(),
        ));
    }
    Ok(PipeStage::Truthy(
        parts.get(1).map(|selector| selector.parse()).transpose()?,
    ))
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

fn selector_stage(
    name: &str,
    parts: &[String],
    build: fn(Selector) -> PipeStage,
) -> Result<PipeStage, PipelineError> {
    require_arg_count(name, parts, 2)?;
    Ok(build(parts[1].parse()?))
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
        .collect::<Result<Vec<_>, _>>()?;

    if columns.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            parts[0]
        )));
    }

    validate_projection_terms(&columns)?;

    Ok(PipeStage::Columns(columns))
}

fn parse_sort_stage_source(stage: &str) -> Option<Result<PipeStage, PipelineError>> {
    let verb_end = stage
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(stage.len());
    let verb = &stage[..verb_end];
    if !matches!(verb, "S" | "sort") {
        return None;
    }

    let source = stage[verb_end..].trim();
    if source.is_empty() {
        return Some(Ok(PipeStage::SortLines { descending: false }));
    }

    Some(parse_sort_source(source))
}

fn parse_sort_source(source: &str) -> Result<PipeStage, PipelineError> {
    let segments = split_sort_keys(source)?;
    if segments.len() == 1 {
        let parts = split(segments[0])
            .ok_or_else(|| PipelineError::Parse("Parsing quoted sort key failed".to_string()))?;
        if parts
            .first()
            .map(|target| target.strip_prefix('!').unwrap_or(target))
            .is_some_and(|target| target == "line")
        {
            return parse_line_sort(&parts);
        }
    }

    let keys = segments
        .into_iter()
        .enumerate()
        .map(|(index, segment)| parse_sort_key(segment, index + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PipeStage::SortColumns(SortSpec::new(keys)?))
}

fn split_sort_keys(source: &str) -> Result<Vec<&str>, PipelineError> {
    let mut keys = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    for (index, ch) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => {}
            None if matches!(ch, '\'' | '"') => quote = Some(ch),
            None if ch == ',' => {
                let key = source[start..index].trim();
                if key.is_empty() {
                    return Err(PipelineError::Parse(
                        "Sort keys cannot be empty".to_string(),
                    ));
                }
                keys.push(key);
                start = index + ch.len_utf8();
            }
            None => {}
        }
    }
    let key = source[start..].trim();
    if key.is_empty() {
        return Err(PipelineError::Parse(
            "Sort keys cannot be empty".to_string(),
        ));
    }
    keys.push(key);
    Ok(keys)
}

fn parse_line_sort(parts: &[String]) -> Result<PipeStage, PipelineError> {
    let (target, prefixed_descending) = parts[0]
        .strip_prefix('!')
        .map_or((parts[0].as_str(), false), |target| (target, true));
    debug_assert_eq!(target, "line");
    let mut position = 1;
    let mut descending = prefixed_descending;
    if let Some(direction) = parts.get(position) {
        if direction.eq_ignore_ascii_case("asc") {
            position += 1;
        } else if direction.eq_ignore_ascii_case("desc") {
            descending = true;
            position += 1;
        }
    }
    if parts
        .get(position)
        .is_some_and(|part| part.eq_ignore_ascii_case("AS"))
    {
        let Some(cast) = parts.get(position + 1) else {
            return Err(PipelineError::Parse(
                "Line sort cast requires a cast name after AS".to_string(),
            ));
        };
        parse_sort_cast(cast)?;
        position += 2;
    }
    if position != parts.len() {
        return Err(PipelineError::Parse(
            "Line sort accepts only line [asc|desc] [AS cast]".to_string(),
        ));
    }
    Ok(PipeStage::SortLines { descending })
}

fn parse_sort_key(source: &str, key_number: usize) -> Result<SortKey, PipelineError> {
    let parts = split(source)
        .ok_or_else(|| PipelineError::Parse(format!("Parsing sort key {key_number} failed")))?;
    let Some(target) = parts.first() else {
        return Err(PipelineError::Parse(format!(
            "Sort key {key_number} cannot be empty"
        )));
    };
    let (target, prefixed_descending) = target
        .strip_prefix('!')
        .map_or((target.as_str(), false), |target| (target, true));
    if target.is_empty() {
        return Err(PipelineError::Parse(format!(
            "Sort key {key_number} requires a selector after !"
        )));
    }

    let mut position = 1;
    let mut direction = if prefixed_descending {
        SortDirection::Descending
    } else {
        SortDirection::Ascending
    };
    if let Some(value) = parts.get(position) {
        if value.eq_ignore_ascii_case("asc") {
            position += 1;
        } else if value.eq_ignore_ascii_case("desc") {
            direction = SortDirection::Descending;
            position += 1;
        }
    }

    let mut cast = SortCast::Auto;
    if parts
        .get(position)
        .is_some_and(|part| part.eq_ignore_ascii_case("AS"))
    {
        let Some(value) = parts.get(position + 1) else {
            return Err(PipelineError::Parse(format!(
                "Sort key {key_number} requires a cast name after AS"
            )));
        };
        cast = parse_sort_cast(value)?;
        position += 2;
    }

    let mut reduction = SortReduction::First;
    if parts
        .get(position)
        .is_some_and(|part| part.eq_ignore_ascii_case("USING"))
    {
        let Some(value) = parts.get(position + 1) else {
            return Err(PipelineError::Parse(format!(
                "Sort key {key_number} requires first, min, or max after USING"
            )));
        };
        reduction = match value.to_ascii_lowercase().as_str() {
            "first" => SortReduction::First,
            "min" => SortReduction::Min,
            "max" => SortReduction::Max,
            _ => {
                return Err(PipelineError::Parse(format!(
                    "Sort key {key_number} has unknown fanout reduction '{value}'"
                )));
            }
        };
        position += 2;
    }

    let mut null_order = NullOrder::Last;
    if parts
        .get(position)
        .is_some_and(|part| part.eq_ignore_ascii_case("NULLS"))
    {
        let Some(value) = parts.get(position + 1) else {
            return Err(PipelineError::Parse(format!(
                "Sort key {key_number} requires FIRST or LAST after NULLS"
            )));
        };
        null_order = match value.to_ascii_lowercase().as_str() {
            "first" => NullOrder::First,
            "last" => NullOrder::Last,
            _ => {
                return Err(PipelineError::Parse(format!(
                    "Sort key {key_number} has unknown null order '{value}'"
                )));
            }
        };
        position += 2;
    }

    if position != parts.len() {
        return Err(PipelineError::Parse(format!(
            "Unexpected token '{}' in sort key {key_number}; expected selector [asc|desc] [AS cast] [USING first|min|max] [NULLS FIRST|LAST]",
            parts[position]
        )));
    }

    Ok(SortKey::new(target)?
        .with_direction(direction)
        .with_cast(cast)
        .with_reduction(reduction)
        .with_null_order(null_order))
}

fn parse_sort_cast(value: &str) -> Result<SortCast, PipelineError> {
    match value.to_ascii_lowercase().as_str() {
        "num" | "number" => Ok(SortCast::Number),
        "str" | "string" => Ok(SortCast::String),
        "bool" | "boolean" => Ok(SortCast::Boolean),
        "ip" => Ok(SortCast::Ip),
        "datetime" => Ok(SortCast::DateTime),
        "version" => Ok(SortCast::Version),
        "natural" => Ok(SortCast::Natural),
        other => Err(PipelineError::Pipe(format!(
            "Unknown sort cast '{other}'. Use str, num, bool, ip, datetime, version, or natural"
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
        keys.push(GroupKey::new(selector, alias)?);
    }

    validate_group_keys(&keys)?;
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
    Ok(PipeStage::Aggregate(AggregateSpec::new(function, alias)?))
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
        "sum" => Ok(AggregateFunction::Sum(field.parse()?)),
        "avg" => Ok(AggregateFunction::Avg(field.parse()?)),
        "min" => Ok(AggregateFunction::Min(field.parse()?)),
        "max" => Ok(AggregateFunction::Max(field.parse()?)),
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
    use crate::model::{
        AggregateFunction, NullOrder, PipeStage, ProjectTerm, SortCast, SortDirection, SortKey,
        SortReduction, SortSpec,
    };

    #[test]
    fn dsl_shorthand_aliases_parse() {
        let (_command, stages) =
            split_pipeline("object list | F active | P name id | S !name | L 5 | C")
                .expect("pipeline");
        assert_eq!(
            stages,
            vec![
                PipeStage::Grep("active".to_string()),
                PipeStage::Columns(vec![
                    ProjectTerm::keep("name").expect("valid selector"),
                    ProjectTerm::keep("id").expect("valid selector"),
                ]),
                PipeStage::SortColumns(
                    SortSpec::new(vec![SortKey::new("name")
                        .expect("valid selector")
                        .with_direction(SortDirection::Descending)])
                    .expect("valid sort"),
                ),
                PipeStage::Head {
                    count: 5,
                    offset: 0
                },
                PipeStage::Count,
            ]
        );
    }

    #[test]
    fn typed_predicates_preserve_quotes_and_legacy_filters() {
        let (_command, stages) = split_pipeline(
            "object list | F WHERE age AS num >= 3 AND (state == \"active\" OR owner IS NULL) | reject WHERE disabled == true",
        )
        .expect("typed predicates");
        assert!(matches!(stages[0], PipeStage::TypedFilter(_)));
        assert!(matches!(stages[1], PipeStage::TypedReject(_)));

        let (_command, stages) =
            split_pipeline("object list | F age=3 | reject retired").expect("legacy filters");
        assert_eq!(stages[0], PipeStage::Grep("age=3".to_string()));
        assert_eq!(stages[1], PipeStage::Reject("retired".to_string()));
    }

    #[test]
    fn multi_key_sorts_parse_every_modifier_in_order() {
        let (_command, stages) = split_pipeline(
            "object list | S state asc, updated_at desc AS datetime NULLS FIRST, data.scores[] AS num USING max NULLS LAST, Name AS natural",
        )
        .expect("multi-key sort");
        let PipeStage::SortColumns(spec) = &stages[0] else {
            panic!("expected structured sort")
        };
        assert_eq!(spec.keys().len(), 4);
        assert_eq!(spec.keys()[0].selector().as_str(), "state");
        assert_eq!(spec.keys()[0].direction(), SortDirection::Ascending);
        assert_eq!(spec.keys()[1].direction(), SortDirection::Descending);
        assert_eq!(spec.keys()[1].cast(), SortCast::DateTime);
        assert_eq!(spec.keys()[1].null_order(), NullOrder::First);
        assert_eq!(spec.keys()[2].reduction(), SortReduction::Max);
        assert_eq!(spec.keys()[3].cast(), SortCast::Natural);
    }

    #[test]
    fn quoted_sort_selectors_and_line_compatibility_parse() {
        let (_, stages) = split_pipeline("object list | S \"OS Version\" desc AS version")
            .expect("quoted selector");
        let PipeStage::SortColumns(spec) = &stages[0] else {
            panic!("expected structured sort")
        };
        assert_eq!(spec.keys()[0].selector().as_str(), "OS Version");

        for source in [
            "object list | S",
            "object list | S line",
            "object list | S !line",
        ] {
            let (_, stages) = split_pipeline(source).expect("line sort compatibility");
            assert!(matches!(stages[0], PipeStage::SortLines { .. }));
        }
    }

    #[test]
    fn malformed_sort_keys_are_rejected_during_parsing() {
        for source in [
            "object list | S state,",
            "object list | S state,,Name",
            "object list | S state AS",
            "object list | S state USING middle",
            "object list | S state NULLS SOMEWHERE",
            "object list | S state desc unexpected",
        ] {
            assert!(split_pipeline(source).is_err(), "{source}");
        }
    }

    #[test]
    fn malformed_typed_predicates_fail_during_pipeline_parsing() {
        for line in [
            "object list | F WHERE",
            "object list | F WHERE age >",
            "object list | F WHERE age IN [1,]",
            "object list | reject WHERE (state == \"active\"",
        ] {
            let error = split_pipeline(line).expect_err("typed predicate should fail");
            assert!(error.to_string().contains("byte"), "{line}: {error}");
        }
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
                if spec.alias() == "Cores"
                    && spec.function()
                        == &AggregateFunction::Sum(
                            "data.cpu.cores".parse().expect("valid selector")
                        )
        ));
    }

    #[test]
    fn unknown_single_letter_stages_fail() {
        assert!(split_pipeline("object list | X thing").is_err());
        assert!(split_pipeline("object list | unknown thing").is_ok());
    }

    #[test]
    fn selector_stages_reject_malformed_selectors_during_parsing() {
        for line in [
            "object list | ? a[bogus]",
            "object list | P a[bogus]",
            "object list | S a[:bogus]",
            "object list | G a[",
            "object list | A sum(a[0]tail)",
            "object list | U a..b",
            "object list | VALUE a]",
            "object list | F a[bogus]=1",
        ] {
            let error = split_pipeline(line).expect_err("selector should fail at parse time");
            assert!(
                error.to_string().contains("Invalid selector"),
                "{line}: {error}"
            );
        }
    }

    #[test]
    fn selector_stages_accept_all_documented_array_forms() {
        for selector in ["a[0]", "a[-1]", "a[]", "a[*]", "a[:2]", "a[1:]"] {
            split_pipeline(&format!("object list | VALUE {selector}"))
                .expect("documented selector should parse");
        }
    }

    #[test]
    fn output_names_must_be_unique_during_parsing() {
        for (line, stage, name) in [
            ("object list | P a a", "P", "a"),
            ("object list | G a AS x b AS x", "G", "x"),
            ("object list | G a AS x | A count AS x", "A", "x"),
            ("object list | G a | A count AS n | A count AS n", "A", "n"),
        ] {
            let error = split_pipeline(line).expect_err("duplicate output name should fail");
            let message = error.to_string();
            assert!(message.contains(&format!("stage '{stage}'")), "{message}");
            assert!(message.contains(name), "{message}");
        }
    }

    #[test]
    fn quoted_output_aliases_with_spaces_remain_valid() {
        let (_, stages) =
            split_pipeline("object list | G os_version AS 'OS Version' | A count AS 'Host Count'")
                .expect("spaced aliases should parse");

        assert!(matches!(
            &stages[..],
            [PipeStage::Group(keys), PipeStage::Aggregate(spec)]
                if keys[0].alias() == "OS Version" && spec.alias() == "Host Count"
        ));
    }

    #[test]
    fn empty_output_aliases_are_rejected() {
        for line in [
            "object list | G name AS ''",
            "object list | G name | A count AS ''",
        ] {
            let error = split_pipeline(line).expect_err("empty alias should fail");
            assert!(error.to_string().contains("output name cannot be empty"));
        }
    }
}
