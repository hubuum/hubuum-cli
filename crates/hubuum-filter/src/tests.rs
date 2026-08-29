use crate::{
    apply_pipeline, group_summary_rows, split_pipeline, validate_jq_expression, AggregateFunction,
    AggregateSpec, GroupKey, OutputEnvelope, OutputShape, PipeStage, ProjectTerm, Selector,
    SortCast,
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
