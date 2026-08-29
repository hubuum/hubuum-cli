use std::str::FromStr;

use regex::Regex;

use crate::model::{validate_group_keys, validate_projection_terms};
use crate::parse::{parse_stage_list, validate_pipeline_output_names};
use crate::verbs::jq::validate_jq_expression;
use crate::verbs::search::validate_filter_expression;
use crate::{
    apply_pipeline, apply_pipeline_with_settings, OutputEnvelope, PipeStage, PipelineError,
    PipelineSettings,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pipeline {
    stages: Vec<PipeStage>,
}

impl Pipeline {
    pub fn parse(source: &str) -> Result<Self, PipelineError> {
        Self::from_stages(parse_stage_list(source)?)
    }

    pub fn from_stages(stages: Vec<PipeStage>) -> Result<Self, PipelineError> {
        for stage in &stages {
            validate_stage(stage)?;
        }
        validate_pipeline_output_names(&stages)?;
        Ok(Self { stages })
    }

    pub fn stages(&self) -> &[PipeStage] {
        &self.stages
    }

    pub fn into_stages(self) -> Vec<PipeStage> {
        self.stages
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn apply(&self, input: OutputEnvelope) -> Result<OutputEnvelope, PipelineError> {
        apply_pipeline(input, &self.stages)
    }

    pub fn apply_with_settings(
        &self,
        input: OutputEnvelope,
        settings: &PipelineSettings,
    ) -> Result<OutputEnvelope, PipelineError> {
        apply_pipeline_with_settings(input, &self.stages, settings)
    }
}

impl FromStr for Pipeline {
    type Err = PipelineError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

fn validate_stage(stage: &PipeStage) -> Result<(), PipelineError> {
    match stage {
        PipeStage::Grep(expression) | PipeStage::Reject(expression) => {
            validate_filter_expression(expression)
        }
        PipeStage::ValueSearch(pattern) | PipeStage::KeySearch(pattern) => {
            Regex::new(pattern).map(|_| ()).map_err(PipelineError::from)
        }
        PipeStage::Columns(terms) => {
            if terms.is_empty() {
                return Err(PipelineError::Pipe(
                    "Pipe stage 'P' requires at least one column".to_string(),
                ));
            }
            validate_projection_terms(terms)
        }
        PipeStage::Group(keys) => {
            if keys.is_empty() {
                return Err(PipelineError::Pipe(
                    "Pipe stage 'G' requires at least one group key".to_string(),
                ));
            }
            validate_group_keys(keys)
        }
        PipeStage::Jq(expression) => validate_jq_expression(expression),
        PipeStage::Truthy(_)
        | PipeStage::TypedFilter(_)
        | PipeStage::TypedReject(_)
        | PipeStage::Head { .. }
        | PipeStage::Tail(_)
        | PipeStage::Count
        | PipeStage::SortLines { .. }
        | PipeStage::SortColumns(_)
        | PipeStage::Aggregate(_)
        | PipeStage::CollapseGroups
        | PipeStage::Unroll(_)
        | PipeStage::Value(_) => Ok(()),
    }
}
