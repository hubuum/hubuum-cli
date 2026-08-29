mod error;
mod eval;
mod model;
mod parse;
mod selector;
mod verb_info;
mod verbs;

#[cfg(test)]
mod tests;

pub use error::PipelineError;
pub use eval::apply_pipeline;
pub use model::{
    AggregateFunction, AggregateSpec, GroupKey, OutputEnvelope, OutputName, OutputShape, PipeStage,
    ProjectTerm, SortCast,
};
pub use parse::split_pipeline;
pub use selector::{scalar_text, select_values, Selector};
pub use verb_info::{help_topics, topic_help, verb_summaries, HelpTopic, VerbSummary};
pub use verbs::collection::group_summary_rows;

pub fn validate_jq_expression(expression: &str) -> Result<(), PipelineError> {
    verbs::jq::validate_jq_expression(expression)
}

#[derive(Debug, Clone, Copy)]
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
    ) -> Self {
        assert!(max_expression_bytes > 0);
        assert!(max_input_bytes > 0);
        assert!(max_outputs > 0);
        assert!(max_output_bytes > 0);
        Self {
            max_expression_bytes,
            max_input_bytes,
            max_outputs,
            max_output_bytes,
        }
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
