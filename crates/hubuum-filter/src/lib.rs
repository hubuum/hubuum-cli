#![doc = include_str!("../README.md")]

mod error;
mod eval;
mod model;
mod parse;
mod pipeline;
mod predicate;
mod selector;
mod settings;
mod value_cast;
mod verbs;

#[cfg(test)]
mod tests;

pub use error::PipelineError;
pub use eval::{apply_pipeline, apply_pipeline_with_settings};
pub use model::{
    AggregateFunction, AggregateSpec, GroupKey, NullOrder, OutputEnvelope, OutputName, OutputShape,
    PipeStage, ProjectTerm, SortCast, SortDirection, SortKey, SortReduction, SortSpec,
};
pub use parse::split_pipeline;
pub use pipeline::Pipeline;
pub use predicate::{
    Comparison, Predicate, PredicateExpr, PredicateOperator, PredicateTest, TypedLiteral, ValueCast,
};
pub use selector::{scalar_text, select_values, Selector};
pub use settings::PipelineSettings;
pub use verbs::collection::group_summary_rows;

pub fn validate_jq_expression(expression: &str) -> Result<(), PipelineError> {
    verbs::jq::validate_jq_expression(expression)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JqLimits {
    max_expression_bytes: usize,
    max_input_bytes: usize,
    max_outputs: usize,
    max_output_bytes: usize,
}

impl JqLimits {
    pub fn new(
        max_expression_bytes: usize,
        max_input_bytes: usize,
        max_outputs: usize,
        max_output_bytes: usize,
    ) -> Result<Self, PipelineError> {
        if [
            max_expression_bytes,
            max_input_bytes,
            max_outputs,
            max_output_bytes,
        ]
        .contains(&0)
        {
            return Err(PipelineError::Jq(
                "JQ expression, input, output-count, and output-byte limits must all be positive"
                    .to_string(),
            ));
        }
        Ok(Self {
            max_expression_bytes,
            max_input_bytes,
            max_outputs,
            max_output_bytes,
        })
    }

    pub fn max_expression_bytes(&self) -> usize {
        self.max_expression_bytes
    }

    pub fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
    }

    pub fn max_outputs(&self) -> usize {
        self.max_outputs
    }

    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }
}

pub fn validate_bounded_jq_expression(
    expression: &str,
    limits: JqLimits,
) -> Result<(), PipelineError> {
    verbs::jq::validate_bounded_jq_expression(expression, limits)
}

pub fn evaluate_bounded_jq(
    input: &serde_json::Value,
    expression: &str,
    limits: JqLimits,
) -> Result<serde_json::Value, PipelineError> {
    verbs::jq::evaluate_bounded_jq(input, expression, limits)
}
