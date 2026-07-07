use jqesque::Jqesque;

use crate::error::PipelineError;
use crate::model::OutputEnvelope;

pub(crate) fn jq_envelope(
    mut envelope: OutputEnvelope,
    expression: &str,
) -> Result<OutputEnvelope, PipelineError> {
    let jqesque = expression
        .parse::<Jqesque>()
        .map_err(|err| PipelineError::Jq(err.to_string()))?;
    jqesque
        .apply_to(&mut envelope.value)
        .map_err(|err| PipelineError::Jq(err.to_string()))?;
    envelope.columns.clear();
    Ok(envelope)
}
