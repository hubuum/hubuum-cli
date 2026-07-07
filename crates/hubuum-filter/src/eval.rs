use regex::Regex;

use crate::error::PipelineError;
use crate::model::{OutputEnvelope, OutputShape, PipeStage, SortCast};
use crate::verbs::{collection, jq, project, search};

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
            | Self::Value(_) => Err(PipelineError::Pipe(
                "Pipe stage requires structured table output".to_string(),
            )),
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
    if envelope.shape == OutputShape::Lines {
        let lines = envelope
            .value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        return Ok(OutputEnvelope::lines(stage.apply(lines)?));
    }

    match stage {
        PipeStage::Grep(pattern) => search::filter_envelope(envelope, pattern, false),
        PipeStage::ValueSearch(pattern) => search::value_search_envelope(envelope, pattern),
        PipeStage::KeySearch(pattern) => search::key_search_envelope(envelope, pattern),
        PipeStage::Truthy(selector) => search::truthy_envelope(envelope, selector.as_deref()),
        PipeStage::Reject(pattern) => search::filter_envelope(envelope, pattern, true),
        PipeStage::Head { count, offset } => {
            collection::limit_envelope(envelope, *count, *offset, false)
        }
        PipeStage::Tail(count) => collection::limit_envelope(envelope, *count, 0, true),
        PipeStage::Count => collection::count_envelope(envelope),
        PipeStage::SortLines { descending } => {
            collection::sort_envelope(envelope, None, *descending, SortCast::Auto)
        }
        PipeStage::Columns(columns) => project::project_envelope(envelope, columns),
        PipeStage::SortColumn {
            column,
            descending,
            cast,
        } => collection::sort_envelope(envelope, Some(column), *descending, *cast),
        PipeStage::Group(keys) => collection::group_envelope(envelope, keys),
        PipeStage::Aggregate(spec) => collection::aggregate_envelope(envelope, spec),
        PipeStage::CollapseGroups => collection::collapse_groups(envelope),
        PipeStage::Unroll(selector) => collection::unroll_envelope(envelope, selector),
        PipeStage::Jq(expression) => jq::jq_envelope(envelope, expression),
        PipeStage::Value(selector) => project::value_envelope(envelope, selector),
    }
}
