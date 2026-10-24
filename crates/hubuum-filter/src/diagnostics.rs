use std::collections::BTreeSet;

use serde_json::Value;
use strsim::damerau_levenshtein;

use crate::pipeline::FilterPlan;
use crate::selector::{key_paths, select_values, Selector};
use crate::verbs::collection::group_summary_rows;
use crate::{
    MissingSelectorPolicy, OutputEnvelope, OutputShape, PipeStage, PipelineError, PipelineSettings,
    PredicateExpr, PredicateOperator,
};

pub(crate) fn validate_selectors(
    input: &OutputEnvelope,
    stage: &PipeStage,
    plan: &FilterPlan,
    settings: &PipelineSettings,
) -> Result<(), PipelineError> {
    if settings.missing_selector_policy() == MissingSelectorPolicy::Allow
        || input.shape() == OutputShape::Lines
    {
        return Ok(());
    }
    let mut selectors = Vec::new();
    let mut guarded = BTreeSet::new();
    match stage {
        PipeStage::TypedFilter(predicate) | PipeStage::TypedReject(predicate) => {
            predicate_selectors(predicate.expression(), &mut selectors, &mut guarded)
        }
        PipeStage::Grep(_) | PipeStage::Reject(_) => selectors.extend(plan.semantic().selector()),
        PipeStage::Columns(terms) | PipeStage::ProjectRows(terms) => selectors.extend(
            terms
                .iter()
                .filter(|term| !term.is_drop())
                .map(|term| term.selector()),
        ),
        PipeStage::SortColumns(spec) => {
            selectors.extend(spec.keys().iter().map(|key| key.selector()))
        }
        PipeStage::Group(keys) => selectors.extend(keys.iter().map(|key| key.selector())),
        PipeStage::Distinct(spec) => selectors.extend(spec.keys().iter().map(|key| key.selector())),
        PipeStage::Unroll(selector) | PipeStage::Value(selector) => selectors.push(selector),
        PipeStage::UnrollInto(spec) => selectors.push(spec.selector()),
        _ => {}
    }
    let summaries;
    let rows: Vec<&Value> = match input.shape() {
        OutputShape::Groups => {
            summaries = group_summary_rows(input.value());
            summaries.iter().collect()
        }
        OutputShape::Rows | OutputShape::Values => {
            input.value().as_array().into_iter().flatten().collect()
        }
        OutputShape::Empty => Vec::new(),
        _ => vec![input.value()],
    };
    if rows.is_empty() {
        return Ok(());
    }
    for selector in selectors {
        if guarded.contains(selector.as_str())
            || rows.iter().any(|row| {
                !select_values(row, selector).is_empty() || selector.has_empty_fanout(row)
            })
        {
            continue;
        }
        let available = rows
            .iter()
            .flat_map(|row| key_paths(row, &PipelineSettings::default()))
            .map(|(path, _)| path)
            .collect::<BTreeSet<_>>();
        let suggestion = available
            .iter()
            .map(|path| (damerau_levenshtein(selector.as_str(), path), path))
            .filter(|(distance, _)| *distance <= 2)
            .min();
        let hint = suggestion
            .map(|(_, path)| format!("; did you mean '{path}'?"))
            .unwrap_or_default();
        return Err(PipelineError::Pipe(format!("Pipe stage '{}' selector '{}' is absent from all {} records; available paths: {}{}. Use CHECK ALLOW for optional fields", stage.name(), selector, rows.len(), available.into_iter().collect::<Vec<_>>().join(", "), hint)));
    }
    Ok(())
}

fn predicate_selectors<'a>(
    expression: &'a PredicateExpr,
    selectors: &mut Vec<&'a Selector>,
    guarded: &mut BTreeSet<&'a str>,
) {
    match expression {
        PredicateExpr::And(a, b) | PredicateExpr::Or(a, b) => {
            predicate_selectors(a, selectors, guarded);
            predicate_selectors(b, selectors, guarded);
        }
        PredicateExpr::Not(inner) => predicate_selectors(inner, selectors, guarded),
        PredicateExpr::Test(test) => {
            if matches!(test.operator(), PredicateOperator::IsMissing { .. }) {
                guarded.insert(test.selector().as_str());
            } else {
                selectors.push(test.selector());
            }
        }
    }
}
