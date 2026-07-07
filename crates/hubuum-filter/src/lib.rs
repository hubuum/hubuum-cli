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
    AggregateFunction, AggregateSpec, GroupKey, OutputEnvelope, OutputShape, PipeStage,
    ProjectTerm, SortCast,
};
pub use parse::split_pipeline;
pub use selector::{scalar_text, select_values};
pub use verb_info::{help_topics, topic_help, verb_summaries, HelpTopic, VerbSummary};
pub use verbs::collection::group_summary_rows;
