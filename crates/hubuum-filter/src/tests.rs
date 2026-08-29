use crate::{
    apply_pipeline, apply_pipeline_with_settings, group_summary_rows, split_pipeline,
    validate_jq_expression, AggregateFunction, AggregateSpec, GroupKey, OutputEnvelope,
    OutputShape, PipeStage, PipelineSettings, ProjectTerm, Selector, SortCast,
};
use serde_json::json;

fn host_rows() -> OutputEnvelope {
    OutputEnvelope::rows(
        vec![
            json!({
                "Name": "host-a",
                "os_version": "26.1",
                "data": {
                    "cpu": {"cores": 8},
                    "network": {"interfaces": [
                        {"ipv4": "129.240.1.10", "mac": "aa"},
                        {"ipv4": "10.0.0.10", "mac": "bb"}
                    ]}
                }
            }),
            json!({
                "Name": "host-b",
                "os_version": "26.1",
                "data": {
                    "cpu": {"cores": 4},
                    "network": {"interfaces": [
                        {"ipv4": "129.240.1.11", "mac": "cc"}
                    ]}
                }
            }),
            json!({
                "Name": "host-c",
                "os_version": "25.9",
                "data": {
                    "cpu": {"cores": 2},
                    "network": {"interfaces": []}
                }
            }),
        ],
        vec!["Name".to_string(), "os_version".to_string()],
    )
}

fn grouped_value_rows() -> OutputEnvelope {
    OutputEnvelope::rows(
        vec![
            json!({"g": "x", "v": 1}),
            json!({"g": "x", "v": 2}),
            json!({"g": "y", "v": 3}),
        ],
        vec!["g".to_string(), "v".to_string()],
    )
}

fn selector(value: &str) -> Selector {
    Selector::new(value).expect("valid selector")
}

fn group_key(selector: &str, alias: &str) -> GroupKey {
    GroupKey::new(selector, alias).expect("valid group key")
}

fn aggregate(function: AggregateFunction, alias: &str) -> AggregateSpec {
    AggregateSpec::new(function, alias).expect("valid aggregate")
}

fn keep(selector: &str) -> ProjectTerm {
    ProjectTerm::keep(selector).expect("valid projection selector")
}

fn drop(selector: &str) -> ProjectTerm {
    ProjectTerm::drop(selector).expect("valid projection selector")
}

fn representative_stages() -> Vec<PipeStage> {
    vec![
        PipeStage::Grep(".*".to_string()),
        PipeStage::ValueSearch(".*".to_string()),
        PipeStage::KeySearch("field".to_string()),
        PipeStage::Truthy(None),
        PipeStage::Reject("does-not-match".to_string()),
        PipeStage::Head {
            count: 1,
            offset: 0,
        },
        PipeStage::Tail(1),
        PipeStage::Count,
        PipeStage::SortLines { descending: false },
        PipeStage::Columns(vec![keep("field")]),
        PipeStage::SortColumn {
            selector: selector("field"),
            descending: false,
            cast: SortCast::Auto,
        },
        PipeStage::Group(vec![group_key("field", "group")]),
        PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
        PipeStage::CollapseGroups,
        PipeStage::Unroll(selector("items")),
        PipeStage::Jq(".".to_string()),
        PipeStage::Value(selector("field")),
    ]
}

fn envelope_for_shape(shape: OutputShape) -> OutputEnvelope {
    let record = json!({"field": 1, "items": [1, 2]});
    match shape {
        OutputShape::Empty => OutputEnvelope::empty(),
        OutputShape::Lines => OutputEnvelope::lines(vec!["line".to_string()]),
        OutputShape::Rows => {
            OutputEnvelope::rows(vec![record], vec!["field".to_string(), "items".to_string()])
        }
        OutputShape::Detail => {
            OutputEnvelope::detail(record, vec!["field".to_string(), "items".to_string()])
        }
        OutputShape::Message => OutputEnvelope::message(record),
        OutputShape::Values => OutputEnvelope::values(vec![record]),
        OutputShape::Groups => OutputEnvelope::groups(
            vec![json!({
                "groups": {"field": 1, "items": [1, 2]},
                "aggregates": {},
                "rows": [{"field": 1}]
            })],
            vec!["field".to_string(), "items".to_string()],
        ),
    }
}

#[test]
fn every_stage_enforces_and_fulfils_its_shape_contract() {
    let shapes = [
        OutputShape::Empty,
        OutputShape::Lines,
        OutputShape::Rows,
        OutputShape::Detail,
        OutputShape::Message,
        OutputShape::Values,
        OutputShape::Groups,
    ];

    for stage in representative_stages() {
        for shape in shapes {
            let contract = stage.resulting_shapes(shape);
            assert_eq!(
                contract.is_ok(),
                stage.accepted_input_shapes().contains(&shape),
                "{} on {shape}",
                stage.name()
            );

            match contract {
                Ok(resulting_shapes) => {
                    let output = apply_pipeline(envelope_for_shape(shape), &[stage.clone()])
                        .unwrap_or_else(|error| {
                            panic!("{} should accept {shape}: {error}", stage.name())
                        });
                    assert!(
                        resulting_shapes.contains(&output.shape),
                        "{} on {shape} produced undocumented {}",
                        stage.name(),
                        output.shape
                    );
                }
                Err(contract_error) => {
                    let evaluation_error =
                        apply_pipeline(envelope_for_shape(shape), &[stage.clone()])
                            .expect_err("unsupported shape should fail");
                    let message = evaluation_error.to_string();
                    assert_eq!(message, contract_error.to_string());
                    assert!(message.contains(&format!("stage '{}'", stage.name())));
                    assert!(message.contains(&shape.to_string()));
                    assert!(message.contains("expected one of:"));
                }
            }
        }
    }
}

#[test]
fn empty_is_identity_only_for_row_preserving_stages() {
    let empty = OutputEnvelope::empty();
    let identity_stages = representative_stages()
        .into_iter()
        .filter(|stage| {
            stage
                .resulting_shapes(OutputShape::Empty)
                .is_ok_and(|shapes| shapes == [OutputShape::Empty])
        })
        .collect::<Vec<_>>();

    assert_eq!(
        identity_stages
            .iter()
            .map(PipeStage::name)
            .collect::<Vec<_>>(),
        vec!["F", "V", "K", "?", "reject", "L", "tail", "S", "P", "S", "U"]
    );
    for stage in identity_stages {
        assert_eq!(
            apply_pipeline(empty.clone(), &[stage]).expect("empty identity"),
            empty
        );
    }
}

#[test]
fn important_shape_transitions_compose_across_multiple_stages() {
    let output = apply_pipeline(
        OutputEnvelope::detail(
            json!({"items": [{"g": "x", "v": [1, 2]}]}),
            vec!["items".to_string()],
        ),
        &[
            PipeStage::Jq(".items".to_string()),
            PipeStage::Unroll(selector("v")),
            PipeStage::Group(vec![group_key("g", "g")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Sum(selector("v")), "total")),
            PipeStage::CollapseGroups,
            PipeStage::Value(selector("total")),
            PipeStage::Count,
        ],
    )
    .expect("valid shape transition chain");

    assert_eq!(output.shape, OutputShape::Values);
    assert_eq!(output.value, json!([1]));
}

#[test]
fn value_and_key_search_have_distinct_scope() {
    let values = apply_pipeline(
        host_rows(),
        &[PipeStage::ValueSearch("129.240".to_string())],
    )
    .expect("value search");
    assert_eq!(values.value.as_array().expect("rows").len(), 2);

    let keys = apply_pipeline(host_rows(), &[PipeStage::KeySearch("ipv4".to_string())])
        .expect("key search");
    assert_eq!(keys.value.as_array().expect("rows").len(), 2);
    assert!(keys.value.to_string().contains("ipv4"));
    assert!(!keys.value.to_string().contains("host-c"));
}

#[test]
fn quick_search_includes_hidden_semantic_values() {
    let filtered = apply_pipeline(host_rows(), &[PipeStage::Grep("129.240".to_string())])
        .expect("quick search");
    let rows = filtered.value.as_array().expect("rows");

    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.get("Match") == Some(&json!("value"))));
}

#[test]
fn ignored_search_keys_are_explicit_and_generic_defaults_search_every_key() {
    let rows = OutputEnvelope::rows(
        vec![json!({"name": "alpha", "created_at": "2026-08-29"})],
        Vec::new(),
    );
    let stage = PipeStage::Grep("2026".to_string());

    assert_eq!(
        apply_pipeline(rows.clone(), &[stage.clone()])
            .expect("generic search")
            .value
            .as_array()
            .expect("rows")
            .len(),
        1
    );

    let settings = PipelineSettings::new()
        .with_ignored_search_keys(["created_at"])
        .expect("valid ignored key");
    assert!(apply_pipeline_with_settings(rows, &[stage], &settings)
        .expect("configured search")
        .is_empty());
    assert_eq!(
        settings.ignored_search_keys().collect::<Vec<_>>(),
        vec!["created_at"]
    );
    assert!(PipelineSettings::new()
        .with_ignored_search_keys([""])
        .is_err());
    assert!(PipelineSettings::new()
        .with_ignored_search_keys(["   "])
        .is_err());
}

#[test]
fn jq_supports_documented_map_projection() {
    let transformed = apply_pipeline(
        host_rows(),
        &[PipeStage::Jq("map({Name, os_version})".to_string())],
    )
    .expect("jq map projection");

    assert_eq!(transformed.shape, OutputShape::Rows);
    assert!(transformed.columns.is_empty());
    assert_eq!(
        transformed.value,
        json!([
            {"Name": "host-a", "os_version": "26.1"},
            {"Name": "host-b", "os_version": "26.1"},
            {"Name": "host-c", "os_version": "25.9"}
        ])
    );
}

#[test]
fn jq_collects_multiple_outputs_as_semantic_values() {
    let transformed = apply_pipeline(host_rows(), &[PipeStage::Jq(".[] | .Name".to_string())])
        .expect("jq value stream");

    assert_eq!(transformed.shape, OutputShape::Values);
    assert_eq!(transformed.value, json!(["host-a", "host-b", "host-c"]));
}

#[test]
fn jq_reports_invalid_expressions() {
    let error = apply_pipeline(host_rows(), &[PipeStage::Jq("map(".to_string())])
        .expect_err("invalid jq should fail");

    assert!(error.to_string().contains("JQ error"));
}

#[test]
fn jq_expressions_can_be_validated_without_runtime_input() {
    validate_jq_expression("map(.id)").expect("valid expression");
    assert!(validate_jq_expression("map(").is_err());
}

#[test]
fn grouping_aggregates_and_count_use_host_examples() {
    let grouped = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "OS Version")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "Hosts")),
        ],
    )
    .expect("group aggregate");

    assert_eq!(grouped.shape, OutputShape::Groups);
    assert!(grouped.value.to_string().contains("OS Version"));
    assert!(grouped.value.to_string().contains("Hosts"));

    let counted = apply_pipeline(grouped, &[PipeStage::Count]).expect("group count");
    assert_eq!(counted.shape, OutputShape::Rows);
    assert!(counted.value.to_string().contains("\"count\":2"));
}

#[test]
fn grouped_output_sorts_by_aggregate_alias() {
    let sorted = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "OS Version")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "Hosts")),
            PipeStage::SortColumn {
                selector: selector("Hosts"),
                descending: true,
                cast: SortCast::Number,
            },
        ],
    )
    .expect("group aggregate sort");

    let rows = group_summary_rows(&sorted.value);
    assert_eq!(rows[0].get("OS Version"), Some(&json!("26.1")));
    assert_eq!(rows[0].get("Hosts"), Some(&json!(2)));
    assert_eq!(rows[1].get("OS Version"), Some(&json!("25.9")));
    assert_eq!(rows[1].get("Hosts"), Some(&json!(1)));
}

#[test]
fn grouped_projection_uses_summary_columns() {
    let projected = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "OS")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "Hosts")),
            PipeStage::Columns(vec![keep("OS")]),
        ],
    )
    .expect("group summary projection");

    assert_eq!(projected.shape, OutputShape::Groups);
    assert_eq!(projected.columns, vec!["OS"]);
    let rows = group_summary_rows(&projected.value);
    assert!(rows.iter().all(|row| row.get("OS").is_some()));
    assert!(rows.iter().all(|row| row.get("Hosts").is_none()));
}

#[test]
fn grouped_filters_use_visible_summaries_without_mutating_members() {
    let grouped = apply_pipeline(
        grouped_value_rows(),
        &[
            PipeStage::Group(vec![group_key("g", "g")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
        ],
    )
    .expect("group values");

    let member_filter = apply_pipeline(grouped.clone(), &[PipeStage::Grep("v=1".to_string())])
        .expect("member fields are not visible after grouping");
    assert!(member_filter.is_empty());

    let summary_filter = apply_pipeline(grouped, &[PipeStage::Grep("n>=2".to_string())])
        .expect("aggregate aliases are visible after grouping");
    let groups = summary_filter.value.as_array().expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(
        group_summary_rows(&summary_filter.value),
        vec![json!({"g": "x", "n": 2})]
    );
    assert_eq!(groups[0]["rows"].as_array().expect("member rows").len(), 2);
}

#[test]
fn grouped_filters_compose_with_repeated_aggregates_count_and_collapse() {
    let filtered = apply_pipeline(
        grouped_value_rows(),
        &[
            PipeStage::Group(vec![group_key("g", "g")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
            PipeStage::Grep("n>=2".to_string()),
            PipeStage::Aggregate(aggregate(AggregateFunction::Sum(selector("v")), "total")),
        ],
    )
    .expect("filter and aggregate groups");
    assert_eq!(
        group_summary_rows(&filtered.value),
        vec![json!({"g": "x", "n": 2, "total": 3.0})]
    );

    let counted = apply_pipeline(filtered.clone(), &[PipeStage::Count]).expect("group count");
    assert_eq!(counted.shape, OutputShape::Rows);
    assert_eq!(
        counted.value,
        json!([{"g": "x", "n": 2, "total": 3.0, "count": 2}])
    );

    let collapsed = apply_pipeline(filtered, &[PipeStage::CollapseGroups]).expect("collapse");
    assert_eq!(collapsed.shape, OutputShape::Rows);
    assert_eq!(collapsed.value, json!([{"g": "x", "n": 2, "total": 3.0}]));
}

#[test]
fn grouped_search_scopes_and_truthiness_use_visible_summaries() {
    let grouped = apply_pipeline(
        grouped_value_rows(),
        &[
            PipeStage::Group(vec![group_key("g", "g")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
        ],
    )
    .expect("group values");

    let values = apply_pipeline(grouped.clone(), &[PipeStage::ValueSearch("2".to_string())])
        .expect("search visible values");
    assert_eq!(
        group_summary_rows(&values.value),
        vec![json!({"g": "x", "n": 2})]
    );

    let keys = apply_pipeline(grouped.clone(), &[PipeStage::KeySearch("^n$".to_string())])
        .expect("project visible keys");
    assert_eq!(
        group_summary_rows(&keys.value),
        vec![json!({"n": 2}), json!({"n": 1})]
    );

    let rejected = apply_pipeline(grouped.clone(), &[PipeStage::Reject("n=1".to_string())])
        .expect("reject visible summaries");
    assert_eq!(
        group_summary_rows(&rejected.value),
        vec![json!({"g": "x", "n": 2})]
    );

    let truthy = apply_pipeline(grouped, &[PipeStage::Truthy(Some(selector("missing")))])
        .expect("truthiness filters groups");
    assert!(truthy.is_empty());
}

#[test]
fn grouped_unroll_transforms_summaries_and_preserves_members() {
    let grouped = OutputEnvelope::groups(
        vec![json!({
            "groups": {"g": "x", "tags": ["a", "b"]},
            "aggregates": {"n": 2},
            "rows": [{"v": 1}, {"v": 2}]
        })],
        vec!["g".to_string(), "tags".to_string(), "n".to_string()],
    );

    let unrolled = apply_pipeline(
        grouped,
        &[
            PipeStage::Unroll(selector("tags")),
            PipeStage::Aggregate(aggregate(AggregateFunction::Sum(selector("v")), "total")),
        ],
    )
    .expect("unroll grouped summaries");

    assert_eq!(
        group_summary_rows(&unrolled.value),
        vec![
            json!({"g": "x", "tags": "a", "n": 2, "total": 3.0}),
            json!({"g": "x", "tags": "b", "n": 2, "total": 3.0})
        ]
    );
    assert!(unrolled
        .value
        .as_array()
        .expect("groups")
        .iter()
        .all(|group| group["rows"].as_array().expect("member rows").len() == 2));
}

#[test]
fn grouping_fans_out_array_selectors() {
    let grouped = apply_pipeline(
        host_rows(),
        &[PipeStage::Group(vec![group_key(
            "data.network.interfaces[].ipv4",
            "IPv4",
        )])],
    )
    .expect("fanout group");

    assert_eq!(grouped.value.as_array().expect("groups").len(), 4);
    assert!(grouped.value.to_string().contains("null"));
}

#[test]
fn projection_droppers_and_value_extraction_still_work() {
    let projected = apply_pipeline(
        host_rows(),
        &[PipeStage::Columns(vec![
            keep("Name"),
            keep("data"),
            drop("data.cpu"),
        ])],
    )
    .expect("project");
    assert!(projected.value.to_string().contains("network"));
    assert!(!projected.value.to_string().contains("cores"));

    let values = apply_pipeline(
        host_rows(),
        &[PipeStage::Value(selector(
            "data.network.interfaces[-1].ipv4",
        ))],
    )
    .expect("value");
    assert_eq!(values.value, json!(["10.0.0.10", "129.240.1.11"]));
}

#[test]
fn projection_exclusions_share_array_selector_semantics() {
    let input = OutputEnvelope::rows(
        vec![json!({
            "a": [
                {"name": "first", "secret": 1},
                {"name": "second", "secret": 2},
                {"name": "third", "secret": 3}
            ]
        })],
        vec!["a".to_string()],
    );

    for exclusion in ["a[].secret", "a[*].secret"] {
        let projected = apply_pipeline(
            input.clone(),
            &[PipeStage::Columns(vec![keep("a"), drop(exclusion)])],
        )
        .expect("fanout exclusion");
        assert_eq!(
            projected.value,
            json!([{"a": [{"name": "first"}, {"name": "second"}, {"name": "third"}]}])
        );
    }

    let indexed = apply_pipeline(
        input.clone(),
        &[PipeStage::Columns(vec![keep("a"), drop("a[-1].secret")])],
    )
    .expect("negative index exclusion");
    assert_eq!(indexed.value[0]["a"][0]["secret"], json!(1));
    assert!(indexed.value[0]["a"][2].get("secret").is_none());

    let sliced = apply_pipeline(
        input,
        &[PipeStage::Columns(vec![keep("a"), drop("a[:2].secret")])],
    )
    .expect("slice exclusion");
    assert!(sliced.value[0]["a"][0].get("secret").is_none());
    assert!(sliced.value[0]["a"][1].get("secret").is_none());
    assert_eq!(sliced.value[0]["a"][2]["secret"], json!(3));
}

#[test]
fn projection_exclusions_work_on_detail_and_group_summaries() {
    let terms = vec![keep("payload"), drop("payload[].secret")];
    let detail = apply_pipeline(
        OutputEnvelope::detail(
            json!({"payload": [{"name": "visible", "secret": true}]}),
            vec!["payload".to_string()],
        ),
        &[PipeStage::Columns(terms.clone())],
    )
    .expect("detail projection");
    assert_eq!(detail.value, json!({"payload": [{"name": "visible"}]}));

    let groups = OutputEnvelope::groups(
        vec![json!({
            "groups": {"group": "one"},
            "aggregates": {"payload": [{"name": "visible", "secret": true}]},
            "rows": []
        })],
        vec!["group".to_string(), "payload".to_string()],
    );
    let projected =
        apply_pipeline(groups, &[PipeStage::Columns(terms)]).expect("group summary projection");
    assert_eq!(
        group_summary_rows(&projected.value),
        vec![json!({"payload": [{"name": "visible"}]})]
    );
}

#[test]
fn missing_projection_exclusions_do_not_change_unrelated_data() {
    let projected = apply_pipeline(
        OutputEnvelope::detail(
            json!({"payload": {"name": "visible"}}),
            vec!["payload".to_string()],
        ),
        &[PipeStage::Columns(vec![
            keep("payload"),
            drop("payload.missing.secret"),
        ])],
    )
    .expect("missing exclusion");

    assert_eq!(projected.value, json!({"payload": {"name": "visible"}}));
}

#[test]
fn programmatic_stages_cannot_create_duplicate_output_names() {
    let duplicate_projection = apply_pipeline(
        host_rows(),
        &[PipeStage::Columns(vec![keep("Name"), keep("Name")])],
    )
    .expect_err("duplicate projection should fail");
    assert!(duplicate_projection
        .to_string()
        .contains("stage 'P' has duplicate output column 'Name'"));

    let duplicate_groups = apply_pipeline(
        host_rows(),
        &[PipeStage::Group(vec![
            group_key("Name", "duplicate"),
            group_key("os_version", "duplicate"),
        ])],
    )
    .expect_err("duplicate group names should fail");
    assert!(duplicate_groups
        .to_string()
        .contains("stage 'G' has duplicate output name 'duplicate'"));

    let group_collision = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "name")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "name")),
        ],
    )
    .expect_err("aggregate should not overwrite group name");
    assert!(group_collision.to_string().contains("output name 'name'"));

    let aggregate_collision = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "OS")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "count")),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "count")),
        ],
    )
    .expect_err("aggregate should not overwrite earlier aggregate");
    assert!(aggregate_collision
        .to_string()
        .contains("output name 'count'"));
}

#[test]
fn parsing_rejects_unknown_single_letter_stages() {
    assert!(split_pipeline("object list --class Hosts | X foo").is_err());
    assert!(split_pipeline("object list --class Hosts | owner").is_ok());
}
