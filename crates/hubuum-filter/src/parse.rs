use std::collections::HashSet;

use shlex::split;

use crate::error::PipelineError;
use crate::model::{
    validate_group_keys, validate_projection_terms, AggregateFunction, AggregateRequest,
    AggregateSpec, DistinctKey, DistinctSpec, GroupKey, NullOrder, PipeStage, ProjectTerm,
    SortCast, SortDirection, SortKey, SortReduction, SortSpec,
};
use crate::predicate::{Predicate, ValueCast};
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
            PipeStage::Aggregate(request) => {
                if request.is_global() {
                    grouped_names = None;
                } else if let Some(names) = &mut grouped_names {
                    for spec in request.specs() {
                        if !names.insert(spec.alias().to_string()) {
                            return Err(PipelineError::Pipe(format!(
                                "Pipe stage 'A' output name '{}' conflicts with a group key or earlier aggregate",
                                spec.alias()
                            )));
                        }
                    }
                }
            }
            PipeStage::Columns(terms) if grouped_names.is_some() => {
                let names = grouped_names.as_ref().expect("group names exist");
                for alias in terms.iter().filter_map(ProjectTerm::alias) {
                    if names.contains(alias) {
                        return Err(PipelineError::Pipe(format!(
                            "Pipe stage 'P' alias '{alias}' conflicts with a group or aggregate output name"
                        )));
                    }
                }
                let keepers = terms
                    .iter()
                    .filter(|term| !term.is_drop())
                    .map(|term| term.output_name().to_string())
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
            | PipeStage::Distinct(_)
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
    if let Some(projection) = parse_projection_stage_source(stage) {
        return projection;
    }
    if let Some(sort) = parse_sort_stage_source(stage) {
        return sort;
    }
    if let Some(distinct) = parse_distinct_stage_source(stage) {
        return distinct;
    }
    if let Some(aggregate) = parse_aggregate_stage_source(stage) {
        return aggregate;
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
        "columns" | "P" => {
            unreachable!("projection stages are parsed from their original source")
        }
        "sort" | "S" => unreachable!("sort stages are parsed from their original source"),
        "distinct" | "D" => {
            unreachable!("distinct stages are parsed from their original source")
        }
        "G" => parse_group_stage(&parts),
        "A" => unreachable!("aggregate stages are parsed from their original source"),
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

fn parse_projection_stage_source(stage: &str) -> Option<Result<PipeStage, PipelineError>> {
    let verb_end = stage
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(stage.len());
    let verb = &stage[..verb_end];
    if !matches!(verb, "P" | "columns") {
        return None;
    }

    let source = stage[verb_end..].trim();
    Some(parse_projection_source(verb, source))
}

fn parse_projection_source(name: &str, source: &str) -> Result<PipeStage, PipelineError> {
    if source.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            name
        )));
    }

    let parts = split(source).ok_or_else(|| {
        PipelineError::Parse("Parsing quoted projection terms failed".to_string())
    })?;
    let has_alias = parts
        .iter()
        .skip(1)
        .any(|part| part.eq_ignore_ascii_case("AS"));
    let columns = if has_alias {
        split_comma_separated(source, "Projection terms")?
            .into_iter()
            .enumerate()
            .map(|(index, term)| parse_projection_term(term, index + 1))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        parts
            .iter()
            .flat_map(|part| part.split(','))
            .map(str::trim)
            .filter(|column| !column.is_empty())
            .map(|column| {
                column
                    .strip_prefix('!')
                    .map(ProjectTerm::drop)
                    .unwrap_or_else(|| ProjectTerm::keep(column))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    if columns.is_empty() {
        return Err(PipelineError::Pipe(format!(
            "Pipe stage '{}' requires at least one column",
            name
        )));
    }

    validate_projection_terms(&columns)?;

    Ok(PipeStage::Columns(columns))
}

fn split_comma_separated<'a>(
    source: &'a str,
    item_description: &str,
) -> Result<Vec<&'a str>, PipelineError> {
    let mut items = Vec::new();
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
                let item = source[start..index].trim();
                if item.is_empty() {
                    return Err(PipelineError::Parse(format!(
                        "{item_description} cannot be empty"
                    )));
                }
                items.push(item);
                start = index + ch.len_utf8();
            }
            None => {}
        }
    }
    let item = source[start..].trim();
    if item.is_empty() {
        return Err(PipelineError::Parse(format!(
            "{item_description} cannot be empty"
        )));
    }
    items.push(item);
    Ok(items)
}

fn parse_projection_term(source: &str, term_number: usize) -> Result<ProjectTerm, PipelineError> {
    let parts = split(source).ok_or_else(|| {
        PipelineError::Parse(format!(
            "Parsing quoted projection term {term_number} failed"
        ))
    })?;
    let Some(selector) = parts.first() else {
        return Err(PipelineError::Parse(format!(
            "Projection term {term_number} cannot be empty"
        )));
    };
    let (selector, drop) = selector
        .strip_prefix('!')
        .map_or((selector.as_str(), false), |selector| (selector, true));

    if parts.len() == 1 {
        return if drop {
            ProjectTerm::drop(selector)
        } else {
            ProjectTerm::keep(selector)
        };
    }
    if parts.len() != 3 || !parts[1].eq_ignore_ascii_case("AS") {
        return Err(PipelineError::Parse(format!(
            "Projection term {term_number} must be selector [AS output-name]; commas are required between terms when any alias is used"
        )));
    }
    if drop {
        return Err(PipelineError::Parse(format!(
            "Projection drop term {term_number} cannot use AS"
        )));
    }
    ProjectTerm::aliased(selector, &parts[2])
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
    let segments = split_comma_separated(source, "Sort keys")?;
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

fn parse_distinct_stage_source(stage: &str) -> Option<Result<PipeStage, PipelineError>> {
    let verb_end = stage
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(stage.len());
    let verb = &stage[..verb_end];
    if !matches!(verb, "D" | "distinct") {
        return None;
    }

    let source = stage[verb_end..].trim();
    if source.is_empty() {
        return Some(Ok(PipeStage::Distinct(DistinctSpec::whole_value())));
    }

    Some(parse_distinct_source(source))
}

fn parse_distinct_source(source: &str) -> Result<PipeStage, PipelineError> {
    let keys = split_comma_separated(source, "Distinct keys")?
        .into_iter()
        .enumerate()
        .map(|(index, key)| parse_distinct_key(key, index + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PipeStage::Distinct(DistinctSpec::by_keys(keys)?))
}

fn parse_distinct_key(source: &str, key_number: usize) -> Result<DistinctKey, PipelineError> {
    let parts = split(source)
        .ok_or_else(|| PipelineError::Parse(format!("Parsing distinct key {key_number} failed")))?;
    let Some(selector) = parts.first() else {
        return Err(PipelineError::Parse(format!(
            "Distinct key {key_number} cannot be empty"
        )));
    };
    let mut key = DistinctKey::new(selector)?;
    if parts.len() == 1 {
        return Ok(key);
    }
    if parts.len() != 3 || !parts[1].eq_ignore_ascii_case("AS") {
        return Err(PipelineError::Parse(format!(
            "Distinct key {key_number} must be selector [AS cast]"
        )));
    }
    let cast = parts[2].parse::<ValueCast>().map_err(|_| {
        PipelineError::Parse(format!(
            "Distinct key {key_number} has unknown cast '{}'. Use str, num, bool, ip, datetime, version, or natural",
            parts[2]
        ))
    })?;
    key = key.with_cast(cast);
    Ok(key)
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

fn parse_aggregate_stage_source(stage: &str) -> Option<Result<PipeStage, PipelineError>> {
    let verb_end = stage
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(stage.len());
    if &stage[..verb_end] != "A" {
        return None;
    }

    let source = stage[verb_end..].trim();
    if source.is_empty() {
        return Some(Err(PipelineError::Pipe(
            "Pipe stage 'A' requires an aggregate expression".to_string(),
        )));
    }

    let Some(global_source) = source.strip_prefix("GLOBAL") else {
        return Some(
            parse_aggregate_term(source)
                .map(|spec| PipeStage::Aggregate(AggregateRequest::grouped(spec))),
        );
    };
    if !global_source.is_empty()
        && !global_source
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    {
        return Some(
            parse_aggregate_term(source)
                .map(|spec| PipeStage::Aggregate(AggregateRequest::grouped(spec))),
        );
    }

    let global_source = global_source.trim();
    if global_source.is_empty() {
        return Some(Err(PipelineError::Pipe(
            "Pipe stage 'A GLOBAL' requires at least one aggregate".to_string(),
        )));
    }
    Some(
        split_comma_separated(global_source, "Global aggregate terms")
            .and_then(|terms| {
                terms
                    .into_iter()
                    .map(parse_aggregate_term)
                    .collect::<Result<Vec<_>, _>>()
            })
            .and_then(AggregateRequest::global)
            .map(PipeStage::Aggregate),
    )
}

fn parse_aggregate_term(source: &str) -> Result<AggregateSpec, PipelineError> {
    let parts = split(source)
        .ok_or_else(|| PipelineError::Parse("Parsing aggregate term failed".to_string()))?;
    if parts.len() != 1 && parts.len() != 3 {
        return Err(PipelineError::Pipe(
            "Pipe stage 'A' accepts aggregate terms as <aggregate> [AS alias]".to_string(),
        ));
    }
    if parts.len() == 3 && !parts[1].eq_ignore_ascii_case("AS") {
        return Err(PipelineError::Pipe(
            "Aggregate alias requires AS <name>".to_string(),
        ));
    }

    let function = parse_aggregate_function(&parts[0])?;
    let alias = parts
        .get(2)
        .cloned()
        .unwrap_or_else(|| default_aggregate_alias(&function));
    AggregateSpec::new(function, alias)
}

fn parse_aggregate_function(value: &str) -> Result<AggregateFunction, PipelineError> {
    if value == "count" {
        return Ok(AggregateFunction::Count);
    }

    let Some((name, rest)) = value.split_once('(') else {
        return Err(PipelineError::Pipe(format!(
            "Unknown aggregate '{value}'. Use count, count(field), count_distinct(field), sum(field), avg(field), min(field), max(field), first(field), or last(field)"
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
        "count" => Ok(AggregateFunction::CountSelected(field.parse()?)),
        "count_distinct" => Ok(AggregateFunction::CountDistinct(field.parse()?)),
        "sum" => Ok(AggregateFunction::Sum(field.parse()?)),
        "avg" => Ok(AggregateFunction::Avg(field.parse()?)),
        "min" => Ok(AggregateFunction::Min(field.parse()?)),
        "max" => Ok(AggregateFunction::Max(field.parse()?)),
        "first" => Ok(AggregateFunction::First(field.parse()?)),
        "last" => Ok(AggregateFunction::Last(field.parse()?)),
        other => Err(PipelineError::Pipe(format!(
            "Unknown aggregate function '{other}'"
        ))),
    }
}

fn default_aggregate_alias(function: &AggregateFunction) -> String {
    match function {
        AggregateFunction::Count => "count".to_string(),
        AggregateFunction::CountSelected(field) => format!("count({field})"),
        AggregateFunction::CountDistinct(field) => format!("count_distinct({field})"),
        AggregateFunction::Sum(field) => format!("sum({field})"),
        AggregateFunction::Avg(field) => format!("avg({field})"),
        AggregateFunction::Min(field) => format!("min({field})"),
        AggregateFunction::Max(field) => format!("max({field})"),
        AggregateFunction::First(field) => format!("first({field})"),
        AggregateFunction::Last(field) => format!("last({field})"),
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
        AggregateFunction, DistinctSpec, NullOrder, PipeStage, ProjectTerm, SortCast,
        SortDirection, SortKey, SortReduction, SortSpec,
    };
    use crate::predicate::ValueCast;

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
    fn projection_aliases_parse_with_mixed_and_quoted_terms() {
        let (_, stages) = split_pipeline(
            "object list | P Name AS Host, data.interfaces[].ip AS 'IP Addresses', state, !secret",
        )
        .expect("aliased projection");
        let PipeStage::Columns(terms) = &stages[0] else {
            panic!("expected projection")
        };

        assert_eq!(terms.len(), 4);
        assert_eq!(terms[0].selector().as_str(), "Name");
        assert_eq!(terms[0].alias(), Some("Host"));
        assert_eq!(terms[1].alias(), Some("IP Addresses"));
        assert_eq!(terms[2].output_name(), "state");
        assert!(terms[3].is_drop());
    }

    #[test]
    fn aliased_projection_terms_require_commas_and_reject_drop_aliases() {
        let missing_comma = split_pipeline("object list | P Name AS Host state AS State")
            .expect_err("aliased terms need commas");
        assert!(missing_comma.to_string().contains("commas are required"));

        let drop_alias = split_pipeline("object list | P Name AS Host, !secret AS Hidden")
            .expect_err("drop aliases must fail");
        assert!(drop_alias.to_string().contains("drop term 2 cannot use AS"));

        for source in [
            "object list | P Name AS Host,",
            "object list | P Name AS Host,, state",
        ] {
            assert!(split_pipeline(source).is_err(), "{source}");
        }
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
    fn whole_value_and_keyed_distinct_stages_parse() {
        for source in ["object list | D", "object list | distinct"] {
            let (_, stages) = split_pipeline(source).expect("whole-value distinct");
            assert!(matches!(
                &stages[0],
                PipeStage::Distinct(spec) if spec == &DistinctSpec::whole_value()
            ));
        }

        let (_, stages) = split_pipeline(
            "object list | D owner, address AS ip, updated_at AS datetime, Name AS natural",
        )
        .expect("keyed distinct");
        let PipeStage::Distinct(spec) = &stages[0] else {
            panic!("expected distinct stage")
        };
        assert_eq!(spec.keys().len(), 4);
        assert_eq!(spec.keys()[0].selector().as_str(), "owner");
        assert_eq!(spec.keys()[1].cast(), Some(ValueCast::Ip));
        assert_eq!(spec.keys()[2].cast(), Some(ValueCast::DateTime));
        assert_eq!(spec.keys()[3].cast(), Some(ValueCast::Natural));
    }

    #[test]
    fn malformed_distinct_keys_fail_during_parsing() {
        for source in [
            "object list | D owner,",
            "object list | D owner,,address",
            "object list | D owner AS",
            "object list | D owner AS unknown",
            "object list | D owner unexpected",
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
            PipeStage::Aggregate(request)
                if request.specs()[0].alias() == "Cores"
                    && request.specs()[0].function()
                        == &AggregateFunction::Sum(
                            "data.cpu.cores".parse().expect("valid selector")
                        )
        ));
    }

    #[test]
    fn global_aggregate_lists_parse_selector_counts_and_quoted_aliases() {
        let (_, stages) = split_pipeline(
            "object list | A GLOBAL count AS Hosts, count(data.owner) AS Owned, count_distinct(os_version) AS 'OS, Versions'",
        )
        .expect("global aggregate list");
        let PipeStage::Aggregate(request) = &stages[0] else {
            panic!("expected aggregate request")
        };

        assert!(request.is_global());
        assert_eq!(request.specs().len(), 3);
        assert_eq!(request.specs()[0].function(), &AggregateFunction::Count);
        assert_eq!(request.specs()[0].alias(), "Hosts");
        assert_eq!(
            request.specs()[1].function(),
            &AggregateFunction::CountSelected("data.owner".parse().expect("valid selector"))
        );
        assert_eq!(
            request.specs()[2].function(),
            &AggregateFunction::CountDistinct("os_version".parse().expect("valid selector"))
        );
        assert_eq!(request.specs()[2].alias(), "OS, Versions");
    }

    #[test]
    fn first_and_last_aggregates_parse_with_default_and_explicit_aliases() {
        let (_, stages) =
            split_pipeline("task events 1 | A GLOBAL first(created_at), last(created_at) AS Last")
                .expect("ordered aggregate list");
        let PipeStage::Aggregate(request) = &stages[0] else {
            panic!("expected aggregate request")
        };

        assert_eq!(request.specs()[0].alias(), "first(created_at)");
        assert_eq!(
            request.specs()[0].function(),
            &AggregateFunction::First("created_at".parse().expect("valid selector"))
        );
        assert_eq!(request.specs()[1].alias(), "Last");
        assert_eq!(
            request.specs()[1].function(),
            &AggregateFunction::Last("created_at".parse().expect("valid selector"))
        );
    }

    #[test]
    fn malformed_global_aggregate_lists_fail_during_parsing() {
        for source in [
            "object list | A GLOBAL",
            "object list | A GLOBAL count,",
            "object list | A GLOBAL count,,count(owner)",
            "object list | A GLOBAL count()",
            "task events 1 | A GLOBAL first()",
            "audit list | A GLOBAL last(occurred_at",
            "object list | A GLOBAL count(owner) AS",
            "object list | A GLOBAL count AS n, count(owner) AS n",
            "object list | A GLOBAL first(created_at) AS Boundary, last(created_at) AS Boundary",
        ] {
            assert!(split_pipeline(source).is_err(), "{source}");
        }
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
            ("object list | P a AS x, b AS x", "P", "x"),
            ("object list | P x, b AS x", "P", "x"),
            ("object list | G a AS x b AS x", "G", "x"),
            ("object list | G a AS x | A count AS x", "A", "x"),
            ("object list | G a | A count AS n | A count AS n", "A", "n"),
            (
                "task events 1 | G kind AS First | A first(created_at) AS First",
                "A",
                "First",
            ),
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
            split_pipeline("object list | P Name AS 'Host, Name' | G os_version AS 'OS Version' | A count AS 'Host Count'")
                .expect("spaced aliases should parse");

        assert!(matches!(
            &stages[0],
            PipeStage::Columns(terms) if terms[0].alias() == Some("Host, Name")
        ));
        assert!(matches!(
            &stages[1..],
            [PipeStage::Group(keys), PipeStage::Aggregate(request)]
                if keys[0].alias() == "OS Version"
                    && request.specs()[0].alias() == "Host Count"
        ));
    }

    #[test]
    fn empty_output_aliases_are_rejected() {
        for line in [
            "object list | P name AS ''",
            "object list | G name AS ''",
            "object list | G name | A count AS ''",
        ] {
            let error = split_pipeline(line).expect_err("empty alias should fail");
            assert!(error.to_string().contains("output name cannot be empty"));
        }
    }

    #[test]
    fn projection_aliases_cannot_overwrite_group_or_aggregate_names() {
        for (line, name) in [
            ("object list | G rack AS Rack | P name AS Rack", "Rack"),
            (
                "object list | G rack AS Rack | A count AS Hosts | P name AS Hosts",
                "Hosts",
            ),
        ] {
            let error = split_pipeline(line).expect_err("grouped alias collision should fail");
            let message = error.to_string();
            assert!(message.contains("stage 'P'"), "{message}");
            assert!(message.contains(name), "{message}");
            assert!(message.contains("group or aggregate"), "{message}");
        }
    }
}
