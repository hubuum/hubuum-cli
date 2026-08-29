use regex::Regex;

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape, PipeStage, SortCast};
use crate::verbs::collection::{
    aggregate_envelope, collapse_groups, count_envelope, group_envelope, limit_envelope,
    sort_envelope, unroll_envelope,
};
use crate::verbs::jq::jq_envelope;
use crate::verbs::project::{project_envelope, value_envelope};
use crate::verbs::search::{
    filter_envelope, key_search_envelope, truthy_envelope, value_search_envelope,
};

impl PipeStage {
    fn apply(&self, lines: Vec<String>) -> Result<Vec<String>, PipelineError> {
        self.validate_input_shape(OutputShape::Lines)?;
        match self {
            Self::Grep(pattern) | Self::ValueSearch(pattern) => {
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
            Self::Head { count, offset } => {
                Ok(lines.into_iter().skip(*offset).take(*count).collect())
            }
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
            Self::KeySearch(_)
            | Self::Truthy(_)
            | Self::Columns(_)
            | Self::SortColumn { .. }
            | Self::Group(_)
            | Self::Aggregate(_)
            | Self::CollapseGroups
            | Self::Unroll(_)
            | Self::Jq(_)
            | Self::Value(_) => unreachable!("line input shape was validated"),
        }
    }
}

pub fn apply_pipeline(
    envelope: OutputEnvelope,
    stages: &[PipeStage],
) -> Result<OutputEnvelope, PipelineError> {
    let mut envelope = envelope;
    for stage in stages {
        envelope = apply_semantic_stage(envelope, stage)?;
    }
    Ok(envelope)
}

fn apply_semantic_stage(
    envelope: OutputEnvelope,
    stage: &PipeStage,
) -> Result<OutputEnvelope, PipelineError> {
    stage.validate_input_shape(envelope.shape)?;
    if envelope.shape == OutputShape::Lines {
        let lines = envelope
            .value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        if matches!(stage, PipeStage::Count) {
            return Ok(OutputEnvelope::values(vec![serde_json::Value::Number(
                lines.len().into(),
            )]));
        }
        return Ok(OutputEnvelope::lines(stage.apply(lines)?));
    }

    match stage {
        PipeStage::Grep(pattern) => filter_envelope(envelope, pattern, false),
        PipeStage::ValueSearch(pattern) => value_search_envelope(envelope, pattern),
        PipeStage::KeySearch(pattern) => key_search_envelope(envelope, pattern),
        PipeStage::Truthy(selector) => truthy_envelope(envelope, selector.as_ref()),
        PipeStage::Reject(pattern) => filter_envelope(envelope, pattern, true),
        PipeStage::Head { count, offset } => limit_envelope(envelope, *count, *offset, false),
        PipeStage::Tail(count) => limit_envelope(envelope, *count, 0, true),
        PipeStage::Count => count_envelope(envelope),
        PipeStage::SortLines { descending } => {
            sort_envelope(envelope, None, *descending, SortCast::Auto)
        }
        PipeStage::Columns(columns) => project_envelope(envelope, columns),
        PipeStage::SortColumn {
            selector,
            descending,
            cast,
        } => sort_envelope(envelope, Some(selector), *descending, *cast),
        PipeStage::Group(keys) => group_envelope(envelope, keys),
        PipeStage::Aggregate(spec) => aggregate_envelope(envelope, spec),
        PipeStage::CollapseGroups => collapse_groups(envelope),
        PipeStage::Unroll(selector) => unroll_envelope(envelope, selector),
        PipeStage::Jq(expression) => jq_envelope(envelope, expression),
        PipeStage::Value(selector) => value_envelope(envelope, selector),
    }
}
