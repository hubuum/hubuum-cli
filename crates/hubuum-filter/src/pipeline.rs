use std::str::FromStr;

use regex::Regex;

use crate::eval::apply_compiled;
use crate::model::{validate_group_keys, validate_projection_terms};
use crate::parse::{parse_stage_list, validate_pipeline_output_names};
use crate::verbs::jq::validate_jq_expression;
use crate::verbs::search::SemanticFilter;
use crate::{OutputEnvelope, PipeStage, PipelineError, PipelineSettings};

#[derive(Debug, Clone, Default)]
pub struct Pipeline {
    stages: Vec<PipeStage>,
    plans: Vec<FilterPlan>,
}

#[derive(Debug, Clone)]
pub(crate) enum FilterPlan {
    None,
    Legacy {
        semantic: SemanticFilter,
        lines: Result<Regex, String>,
    },
    Regex(Regex),
}

impl FilterPlan {
    fn compile(stage: &PipeStage) -> Result<Self, PipelineError> {
        Ok(match stage {
            PipeStage::Grep(pattern) | PipeStage::Reject(pattern) => Self::Legacy {
                semantic: SemanticFilter::parse(pattern)?,
                lines: Regex::new(pattern).map_err(|error| error.to_string()),
            },
            PipeStage::ValueSearch(pattern) | PipeStage::KeySearch(pattern) => {
                Self::Regex(Regex::new(pattern)?)
            }
            _ => Self::None,
        })
    }

    pub(crate) fn semantic(&self) -> &SemanticFilter {
        match self {
            Self::Legacy { semantic, .. } => semantic,
            _ => unreachable!("compiled legacy filter"),
        }
    }

    pub(crate) fn regex(&self) -> Result<&Regex, PipelineError> {
        match self {
            Self::Regex(regex)
            | Self::Legacy {
                lines: Ok(regex), ..
            } => Ok(regex),
            Self::Legacy {
                lines: Err(error), ..
            } => Err(PipelineError::Parse(error.clone())),
            Self::None => unreachable!("compiled regex filter"),
        }
    }
}

impl PartialEq for Pipeline {
    fn eq(&self, other: &Self) -> bool {
        self.stages == other.stages
    }
}
impl Eq for Pipeline {}

impl Pipeline {
    pub fn parse(source: &str) -> Result<Self, PipelineError> {
        Self::from_stages(parse_stage_list(source)?)
    }

    pub fn from_stages(stages: Vec<PipeStage>) -> Result<Self, PipelineError> {
        for stage in &stages {
            validate_stage(stage)?;
        }
        validate_pipeline_output_names(&stages)?;
        let plans = stages
            .iter()
            .map(FilterPlan::compile)
            .collect::<Result<_, _>>()?;
        Ok(Self { stages, plans })
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
        self.apply_with_settings(input, &PipelineSettings::default())
    }

    pub fn apply_with_settings(
        &self,
        input: OutputEnvelope,
        settings: &PipelineSettings,
    ) -> Result<OutputEnvelope, PipelineError> {
        apply_compiled(input, &self.stages, &self.plans, settings)
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
        PipeStage::Columns(terms) | PipeStage::ProjectRows(terms) => {
            if terms.is_empty() {
                return Err(PipelineError::Pipe(
                    "Pipe stage 'P' requires at least one column".to_string(),
                ));
            }
            if matches!(stage, PipeStage::ProjectRows(_)) && terms.iter().any(|term| term.is_drop())
            {
                return Err(PipelineError::Pipe(
                    "P ROWS accepts keep selectors only".into(),
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
        PipeStage::Grep(_)
        | PipeStage::Reject(_)
        | PipeStage::ValueSearch(_)
        | PipeStage::KeySearch(_)
        | PipeStage::Help(_)
        | PipeStage::SelectorPolicy(_)
        | PipeStage::UnrollInto(_)
        | PipeStage::Truthy(_)
        | PipeStage::TypedFilter(_)
        | PipeStage::TypedReject(_)
        | PipeStage::Head { .. }
        | PipeStage::Tail(_)
        | PipeStage::Count
        | PipeStage::SortLines { .. }
        | PipeStage::SortColumns(_)
        | PipeStage::Distinct(_)
        | PipeStage::Aggregate(_)
        | PipeStage::CollapseGroups
        | PipeStage::Unroll(_)
        | PipeStage::Value(_) => Ok(()),
    }
}
