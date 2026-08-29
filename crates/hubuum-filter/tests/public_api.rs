use hubuum_filter::{
    DistinctKey, DistinctSpec, JqLimits, NullOrder, OutputEnvelope, OutputShape, PipeStage,
    Pipeline, PipelineSettings, ProjectTerm, SortCast, SortDirection, SortKey, SortReduction,
    SortSpec, ValueCast,
};
use serde_json::json;

#[test]
fn parsed_pipeline_is_usable_without_cli_types() {
    let pipeline: Pipeline = "| F active | P name | S name"
        .parse()
        .expect("valid pipeline");
    let input = OutputEnvelope::rows(
        vec![
            json!({"name": "beta", "state": "active"}),
            json!({"name": "alpha", "state": "active"}),
            json!({"name": "retired", "state": "disabled"}),
        ],
        vec!["name".to_string(), "state".to_string()],
    );

    let output = pipeline.apply(input).expect("pipeline output");

    assert_eq!(pipeline.stages().len(), 3);
    assert_eq!(output.shape(), OutputShape::Rows);
    assert_eq!(output.columns(), ["name"]);
    assert_eq!(
        output.value(),
        &json!([{"name": "alpha"}, {"name": "beta"}])
    );
}

#[test]
fn typed_predicates_are_reusable_through_the_public_pipeline_api() {
    let pipeline =
        Pipeline::parse("F WHERE data.cores AS num >= 8 AND state IN [\"ready\", \"running\"]")
            .expect("valid typed predicate");
    let input = OutputEnvelope::rows(
        vec![
            json!({"name": "alpha", "state": "ready", "data": {"cores": "16"}}),
            json!({"name": "beta", "state": "ready", "data": {"cores": "4"}}),
            json!({"name": "gamma", "state": "retired", "data": {"cores": "32"}}),
        ],
        vec!["name".to_string(), "state".to_string()],
    );

    let output = pipeline.apply(input).expect("typed pipeline output");

    assert_eq!(
        output.value(),
        &json!([{
            "name": "alpha",
            "state": "ready",
            "data": {"cores": "16"}
        }])
    );
}

#[test]
fn caller_settings_replace_application_specific_search_policy() {
    let pipeline = Pipeline::parse("F 2026").expect("valid pipeline");
    let input = OutputEnvelope::rows(
        vec![json!({"name": "alpha", "created_at": "2026-08-29"})],
        Vec::new(),
    );
    let settings = PipelineSettings::new()
        .with_ignored_search_keys(["created_at"])
        .expect("valid settings");

    assert!(pipeline
        .apply_with_settings(input, &settings)
        .expect("pipeline output")
        .is_empty());
}

#[test]
fn programmatic_pipeline_construction_validates_public_inputs() {
    let invalid_regex = Pipeline::from_stages(vec![PipeStage::ValueSearch("[".to_string())]);
    assert!(invalid_regex.is_err());

    let invalid_projection = Pipeline::from_stages(vec![PipeStage::Columns(Vec::new())]);
    assert!(invalid_projection.is_err());

    let valid = Pipeline::from_stages(vec![PipeStage::Columns(vec![
        ProjectTerm::keep("name").expect("valid selector")
    ])])
    .expect("valid programmatic pipeline");
    assert_eq!(valid.into_stages().len(), 1);

    let aliased = ProjectTerm::aliased("name", "Host").expect("valid alias");
    assert_eq!(aliased.selector().as_str(), "name");
    assert_eq!(aliased.alias(), Some("Host"));
    assert_eq!(aliased.output_name(), "Host");
    let valid = Pipeline::from_stages(vec![PipeStage::Columns(vec![aliased])])
        .expect("valid programmatic alias pipeline");
    assert_eq!(valid.stages()[0].name(), "P");

    assert!(SortSpec::new(Vec::new()).is_err());
    let sort = SortSpec::new(vec![SortKey::new("scores[]")
        .expect("valid selector")
        .with_direction(SortDirection::Descending)
        .with_cast(SortCast::Number)
        .with_reduction(SortReduction::Max)
        .with_null_order(NullOrder::Last)])
    .expect("validated sort spec");
    let valid = Pipeline::from_stages(vec![PipeStage::SortColumns(sort)])
        .expect("valid programmatic sort pipeline");
    assert_eq!(valid.stages()[0].name(), "S");

    assert!(DistinctSpec::by_keys(Vec::new()).is_err());
    let distinct = DistinctSpec::by_keys(vec![DistinctKey::new("address")
        .expect("valid selector")
        .with_cast(ValueCast::Ip)])
    .expect("valid keyed distinct");
    assert_eq!(distinct.keys()[0].cast(), Some(ValueCast::Ip));
    let valid = Pipeline::from_stages(vec![PipeStage::Distinct(distinct)])
        .expect("valid programmatic distinct pipeline");
    assert_eq!(valid.stages()[0].name(), "D");
}

#[test]
fn jq_limits_are_validated_and_observable() {
    assert!(JqLimits::new(0, 1, 1, 1).is_err());

    let limits = JqLimits::new(128, 1_024, 16, 4_096).expect("positive limits");
    assert_eq!(limits.max_expression_bytes(), 128);
    assert_eq!(limits.max_input_bytes(), 1_024);
    assert_eq!(limits.max_outputs(), 16);
    assert_eq!(limits.max_output_bytes(), 4_096);
}
