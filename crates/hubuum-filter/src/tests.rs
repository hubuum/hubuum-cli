use crate::{
    apply_pipeline, apply_pipeline_with_settings, group_summary_rows, split_pipeline,
    validate_jq_expression, AggregateFunction, AggregateRequest, AggregateSpec, DistinctKey,
    DistinctSpec, GroupKey, OutputEnvelope, OutputShape, PipeStage, PipelineSettings, Predicate,
    ProjectTerm, Selector, SortCast, SortDirection, SortKey, SortSpec,
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

fn aggregate(function: AggregateFunction, alias: &str) -> AggregateRequest {
    AggregateRequest::grouped(AggregateSpec::new(function, alias).expect("valid aggregate"))
}

fn keep(selector: &str) -> ProjectTerm {
    ProjectTerm::keep(selector).expect("valid projection selector")
}

fn drop(selector: &str) -> ProjectTerm {
    ProjectTerm::drop(selector).expect("valid projection selector")
}

fn alias(selector: &str, output_name: &str) -> ProjectTerm {
    ProjectTerm::aliased(selector, output_name).expect("valid projection alias")
}

fn apply_dsl(
    envelope: OutputEnvelope,
    source: &str,
) -> Result<OutputEnvelope, crate::PipelineError> {
    let (_, stages) = split_pipeline(&format!("command | {source}"))?;
    apply_pipeline(envelope, &stages)
}

fn output_ids(envelope: &OutputEnvelope) -> Vec<&str> {
    envelope
        .value
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["id"].as_str().expect("string id"))
        .collect()
}

fn representative_stages() -> Vec<PipeStage> {
    vec![
        PipeStage::Grep(".*".to_string()),
        PipeStage::TypedFilter(Predicate::parse("field == 1").expect("valid predicate")),
        PipeStage::ValueSearch(".*".to_string()),
        PipeStage::KeySearch("field".to_string()),
        PipeStage::Truthy(None),
        PipeStage::Reject("does-not-match".to_string()),
        PipeStage::TypedReject(Predicate::parse("field == 0").expect("valid predicate")),
        PipeStage::Head {
            count: 1,
            offset: 0,
        },
        PipeStage::Tail(1),
        PipeStage::Count,
        PipeStage::SortLines { descending: false },
        PipeStage::Columns(vec![keep("field")]),
        PipeStage::SortColumns(
            SortSpec::new(vec![SortKey::new("field").expect("valid sort selector")])
                .expect("valid sort"),
        ),
        PipeStage::Distinct(DistinctSpec::whole_value()),
        PipeStage::Distinct(
            DistinctSpec::by_keys(vec![DistinctKey::new("field").expect("valid distinct key")])
                .expect("keyed distinct"),
        ),
        PipeStage::Group(vec![group_key("field", "group")]),
        PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
        PipeStage::Aggregate(
            AggregateRequest::global(vec![
                AggregateSpec::new(AggregateFunction::Count, "n").expect("valid aggregate")
            ])
            .expect("valid global aggregate"),
        ),
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
                    let output =
                        apply_pipeline(envelope_for_shape(shape), std::slice::from_ref(&stage))
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
                        apply_pipeline(envelope_for_shape(shape), std::slice::from_ref(&stage))
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
        vec![
            "F",
            "F WHERE",
            "V",
            "K",
            "?",
            "reject",
            "reject WHERE",
            "L",
            "tail",
            "S",
            "P",
            "S",
            "D",
            "D",
            "U"
        ]
    );
    for stage in identity_stages {
        assert_eq!(
            apply_pipeline(empty.clone(), &[stage]).expect("empty identity"),
            empty
        );
    }
}

#[test]
fn whole_value_distinct_is_stable_and_preserves_collection_metadata() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"name": "alpha", "nested": {"a": 1, "b": 2}}),
            json!({"nested": {"b": 2, "a": 1}, "name": "alpha"}),
            json!({"name": "beta", "nested": {"a": 1, "b": 2}}),
        ],
        vec!["name".to_string(), "nested".to_string()],
    );
    let distinct = apply_dsl(rows, "D").expect("whole row distinct");
    assert_eq!(distinct.shape, OutputShape::Rows);
    assert_eq!(distinct.columns, ["name", "nested"]);
    assert_eq!(distinct.value.as_array().expect("rows").len(), 2);
    assert_eq!(distinct.value[0]["name"], json!("alpha"));
    assert_eq!(distinct.value[1]["name"], json!("beta"));

    let values = apply_dsl(
        OutputEnvelope::values(vec![json!(1), json!(1), json!(2), json!(1)]),
        "distinct",
    )
    .expect("whole value distinct");
    assert_eq!(values.shape, OutputShape::Values);
    assert_eq!(values.value, json!([1, 2]));

    let lines = apply_dsl(
        OutputEnvelope::lines(vec![
            "alpha".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
        ]),
        "D",
    )
    .expect("line distinct");
    assert_eq!(lines.shape, OutputShape::Lines);
    assert_eq!(lines.value, json!(["alpha", "beta"]));

    assert_eq!(
        apply_dsl(OutputEnvelope::empty(), "D").expect("empty distinct"),
        OutputEnvelope::empty()
    );
}

#[test]
fn keyed_distinct_uses_ordered_tuples_complete_fanout_and_missing_sentinel() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"id": "first", "state": "up", "tags": [1, 2], "owner": "ops"}),
            json!({"id": "duplicate", "state": "up", "tags": [1, 2], "owner": "ops"}),
            json!({"id": "reordered", "state": "up", "tags": [2, 1], "owner": "ops"}),
            json!({"id": "missing", "state": "up"}),
            json!({"id": "missing-again", "state": "up"}),
            json!({"id": "null", "state": "up", "owner": null}),
        ],
        vec!["id".to_string(), "state".to_string(), "tags".to_string()],
    );

    let distinct = apply_dsl(rows, "D state, tags[], owner").expect("keyed distinct");
    assert_eq!(
        output_ids(&distinct),
        ["first", "reordered", "missing", "null"]
    );
    assert_eq!(distinct.columns, ["id", "state", "tags"]);
}

#[test]
fn distinct_reuses_every_strict_cast_and_reports_context() {
    let cases = [
        ("value AS str", json!(1), json!("1")),
        ("value AS num", json!(1), json!("1")),
        ("value AS bool", json!(true), json!("TRUE")),
        (
            "value AS ip",
            json!("2001:0db8:0000:0000:0000:0000:0000:0001"),
            json!("2001:db8::1"),
        ),
        (
            "value AS datetime",
            json!("2026-01-01T00:00:00Z"),
            json!("2026-01-01T01:00:00+01:00"),
        ),
        ("value AS version", json!("1.2.3"), json!("1.2.3")),
        ("value AS natural", json!("host-2"), json!("host-2")),
    ];
    for (key, first, duplicate) in cases {
        let rows = OutputEnvelope::rows(
            vec![
                json!({"id": "first", "value": first}),
                json!({"id": "duplicate", "value": duplicate}),
            ],
            vec!["id".to_string(), "value".to_string()],
        );
        let distinct = apply_dsl(rows, &format!("D {key}")).expect("cast distinct");
        assert_eq!(output_ids(&distinct), ["first"], "{key}");
    }

    let error = apply_dsl(
        OutputEnvelope::rows(
            vec![
                json!({"address": "192.0.2.1"}),
                json!({"address": "192.0.2.999"}),
            ],
            vec!["address".to_string()],
        ),
        "D address AS ip",
    )
    .expect_err("invalid cast must fail");
    let message = error.to_string();
    assert!(message.contains("distinct key 1"), "{message}");
    assert!(message.contains("selector 'address'"), "{message}");
    assert!(message.contains("row 2"), "{message}");
    assert!(message.contains("192.0.2.999"), "{message}");
}

#[test]
fn grouped_distinct_sees_summaries_and_never_merges_members() {
    let groups = OutputEnvelope::groups(
        vec![
            json!({
                "groups": {"rack": "a"},
                "aggregates": {"count": 1},
                "rows": [{"id": "first"}]
            }),
            json!({
                "groups": {"rack": "a"},
                "aggregates": {"count": 1},
                "rows": [{"id": "hidden-duplicate"}, {"id": "not-merged"}]
            }),
            json!({
                "groups": {"rack": "b"},
                "aggregates": {"count": 1},
                "rows": [{"id": "second"}]
            }),
        ],
        vec!["rack".to_string(), "count".to_string()],
    );

    for source in ["D", "D rack, count AS num"] {
        let distinct = apply_dsl(groups.clone(), source).expect("group distinct");
        assert_eq!(distinct.shape, OutputShape::Groups);
        assert_eq!(distinct.columns, ["rack", "count"]);
        assert_eq!(
            group_summary_rows(&distinct.value),
            [
                json!({"rack": "a", "count": 1}),
                json!({"rack": "b", "count": 1}),
            ]
        );
        let retained = distinct.value.as_array().expect("groups");
        assert_eq!(retained[0]["rows"], json!([{"id": "first"}]));
        assert_eq!(retained[1]["rows"], json!([{"id": "second"}]));
    }
}

#[test]
fn distinct_rejects_singleton_shapes_and_keyed_lines_explicitly() {
    for envelope in [
        OutputEnvelope::detail(json!({"id": 1}), vec!["id".to_string()]),
        OutputEnvelope::message(json!({"id": 1})),
    ] {
        let shape = envelope.shape;
        let error = apply_dsl(envelope, "D").expect_err("singleton distinct must fail");
        let message = error.to_string();
        assert!(message.contains("stage 'D'"), "{message}");
        assert!(message.contains(&shape.to_string()), "{message}");
    }

    let error = apply_dsl(OutputEnvelope::lines(vec!["a".to_string()]), "D line")
        .expect_err("keyed line distinct must fail");
    assert!(error.to_string().contains("stage 'D'"));
    assert!(error.to_string().contains("Lines"));
}

#[test]
fn typed_predicates_filter_rows_values_details_messages_and_groups() {
    let predicate = Predicate::parse(
        "data.cpu.cores AS num >= 4 AND (os_version == \"26.1\" OR Name == \"host-c\")",
    )
    .expect("valid predicate");
    let rows = apply_pipeline(host_rows(), &[PipeStage::TypedFilter(predicate)])
        .expect("typed row filter");
    assert_eq!(
        rows.value
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["Name"].as_str().expect("name"))
            .collect::<Vec<_>>(),
        vec!["host-a", "host-b"]
    );

    let values = apply_pipeline(
        OutputEnvelope::values(vec![json!({"n": 1}), json!({"n": 2})]),
        &[PipeStage::TypedFilter(
            Predicate::parse("n IN [2, 3]").expect("valid predicate"),
        )],
    )
    .expect("typed values filter");
    assert_eq!(values.value, json!([{"n": 2}]));

    for envelope in [
        OutputEnvelope::detail(json!({"state": "active"}), vec!["state".to_string()]),
        OutputEnvelope::message(json!({"state": "active"})),
    ] {
        let output = apply_pipeline(
            envelope,
            &[PipeStage::TypedFilter(
                Predicate::parse("state == \"active\"").expect("valid predicate"),
            )],
        )
        .expect("typed singleton filter");
        assert!(!output.is_empty());
    }

    let grouped = apply_pipeline(
        grouped_value_rows(),
        &[
            PipeStage::Group(vec![group_key("g", "g")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "n")),
            PipeStage::TypedFilter(Predicate::parse("n >= 2").expect("valid predicate")),
        ],
    )
    .expect("typed group filter");
    assert_eq!(
        group_summary_rows(&grouped.value),
        vec![json!({"g": "x", "n": 2})]
    );
    assert_eq!(
        grouped.value[0]["rows"].as_array().expect("members").len(),
        2
    );
}

#[test]
fn typed_reject_negates_the_complete_predicate() {
    let output = apply_pipeline(
        host_rows(),
        &[PipeStage::TypedReject(
            Predicate::parse("os_version == \"26.1\" OR data.cpu.cores AS num < 3")
                .expect("valid predicate"),
        )],
    )
    .expect("typed reject");
    assert!(output.is_empty());
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
        apply_pipeline(rows.clone(), std::slice::from_ref(&stage))
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
fn global_aggregation_counts_inventory_cardinality_and_fanout_occurrences() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({
                "Name": "host-a",
                "owner": "ops",
                "os": {"major": 26, "minor": 1},
                "interfaces": [{"address": "192.0.2.1"}, {"address": "192.0.2.2"}]
            }),
            json!({
                "Name": "host-b",
                "owner": null,
                "os": {"minor": 1, "major": 26},
                "interfaces": [{"address": "192.0.2.1"}, {"address": null}]
            }),
            json!({
                "Name": "host-c",
                "os": {"major": 27},
                "interfaces": []
            }),
        ],
        vec!["Name".to_string(), "owner".to_string(), "os".to_string()],
    );

    let output = apply_dsl(
        rows,
        "A GLOBAL count AS Hosts, count(owner) AS Owned, count(interfaces[].address) AS Addresses, count_distinct(os) AS Versions, count_distinct(interfaces[].address) AS UniqueAddresses",
    )
    .expect("global inventory aggregates");

    assert_eq!(output.shape, OutputShape::Rows);
    assert_eq!(
        output.columns,
        ["Hosts", "Owned", "Addresses", "Versions", "UniqueAddresses"]
    );
    assert_eq!(
        output.value,
        json!([{
            "Hosts": 3,
            "Owned": 1,
            "Addresses": 3,
            "Versions": 2,
            "UniqueAddresses": 2
        }])
    );
}

#[test]
fn global_aggregation_returns_one_row_for_empty_and_values_inputs() {
    let empty = apply_dsl(
        OutputEnvelope::empty(),
        "A GLOBAL count AS Rows, count(owner) AS Owners, count_distinct(owner) AS UniqueOwners, sum(cost) AS Cost",
    )
    .expect("empty global aggregate");
    assert_eq!(empty.shape, OutputShape::Rows);
    assert_eq!(empty.columns, ["Rows", "Owners", "UniqueOwners", "Cost"]);
    assert_eq!(
        empty.value,
        json!([{"Rows": 0, "Owners": 0, "UniqueOwners": 0, "Cost": null}])
    );

    let values = apply_dsl(
        OutputEnvelope::values(vec![json!({"owner": "ops"}), json!({"owner": null})]),
        "A GLOBAL count AS Values, count(owner) AS Owners",
    )
    .expect("value global aggregate");
    assert_eq!(values.shape, OutputShape::Rows);
    assert_eq!(values.value, json!([{"Values": 2, "Owners": 1}]));
}

#[test]
fn selector_counts_share_grouped_and_global_reducers() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"rack": "a", "addresses": ["192.0.2.1", "192.0.2.2"]}),
            json!({"rack": "a", "addresses": ["192.0.2.1", null]}),
            json!({"rack": "b"}),
        ],
        vec!["rack".to_string(), "addresses".to_string()],
    );

    let grouped = apply_dsl(
        rows.clone(),
        "G rack | A count(addresses[]) AS Addresses | A count_distinct(addresses[]) AS Unique | Z",
    )
    .expect("grouped selector counts");
    assert_eq!(
        grouped.value,
        json!([
            {"rack": "a", "Addresses": 3, "Unique": 2},
            {"rack": "b", "Addresses": 0, "Unique": 0}
        ])
    );

    let global = apply_dsl(
        rows,
        "A GLOBAL count(addresses[]) AS Addresses, count_distinct(addresses[]) AS Unique",
    )
    .expect("global selector counts");
    assert_eq!(global.value, json!([{"Addresses": 3, "Unique": 2}]));
}

#[test]
fn global_aggregation_rejects_non_collection_shapes_explicitly() {
    for envelope in [
        OutputEnvelope::lines(vec!["line".to_string()]),
        OutputEnvelope::detail(json!({"owner": "ops"}), vec!["owner".to_string()]),
        OutputEnvelope::message(json!({"owner": "ops"})),
        OutputEnvelope::groups(Vec::new(), Vec::new()),
    ] {
        let shape = envelope.shape;
        let error = apply_dsl(envelope, "A GLOBAL count AS n")
            .expect_err("unsupported global aggregation shape");
        let message = error.to_string();
        assert!(message.contains("stage 'A GLOBAL'"), "{message}");
        assert!(message.contains(&shape.to_string()), "{message}");
        assert!(message.contains("Empty, Rows, Values"), "{message}");
    }
}

#[test]
fn grouped_output_sorts_by_aggregate_alias() {
    let sorted = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![group_key("os_version", "OS Version")]),
            PipeStage::Aggregate(aggregate(AggregateFunction::Count, "Hosts")),
            PipeStage::SortColumns(
                SortSpec::new(vec![SortKey::new("Hosts")
                    .expect("valid sort selector")
                    .with_direction(SortDirection::Descending)
                    .with_cast(SortCast::Number)])
                .expect("valid sort"),
            ),
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
fn projection_aliases_preserve_zero_one_and_many_cardinality() {
    let projected = apply_pipeline(
        OutputEnvelope::rows(
            vec![
                json!({
                    "name": "alpha",
                    "interfaces": [{"ip": "192.0.2.1"}, {"ip": "2001:db8::1"}]
                }),
                json!({"name": "beta", "interfaces": []}),
            ],
            vec!["name".to_string(), "interfaces".to_string()],
        ),
        &[PipeStage::Columns(vec![
            alias("name", "Host"),
            alias("interfaces[].ip", "Addresses"),
            alias("owner", "Owner"),
        ])],
    )
    .expect("aliased projection");

    assert_eq!(projected.columns, ["Host", "Addresses", "Owner"]);
    assert_eq!(
        projected.value,
        json!([
            {
                "Host": "alpha",
                "Addresses": ["192.0.2.1", "2001:db8::1"],
                "Owner": null
            },
            {"Host": "beta", "Addresses": null, "Owner": null}
        ])
    );

    let detail = apply_dsl(
        OutputEnvelope::detail(
            json!({"name": "gamma", "interfaces": [{"ip": "192.0.2.3"}]}),
            vec!["name".to_string(), "interfaces".to_string()],
        ),
        "columns name AS Host, interfaces[].ip AS Addresses, owner AS Owner",
    )
    .expect("long-form detail alias projection");
    assert_eq!(detail.shape, OutputShape::Detail);
    assert_eq!(detail.columns, ["Host", "Addresses", "Owner"]);
    assert_eq!(
        detail.value,
        json!({"Host": "gamma", "Addresses": "192.0.2.3", "Owner": null})
    );
}

#[test]
fn grouped_projection_aliases_keep_member_rows_attached() {
    let projected = apply_dsl(
        grouped_value_rows(),
        "G g AS Group | A count AS Members | P Group AS Category, Members AS Count",
    )
    .expect("grouped alias projection");

    assert_eq!(projected.shape, OutputShape::Groups);
    assert_eq!(projected.columns, ["Category", "Count"]);
    assert_eq!(
        group_summary_rows(&projected.value),
        [
            json!({"Category": "x", "Count": 2}),
            json!({"Category": "y", "Count": 1}),
        ]
    );
    let groups = projected.value.as_array().expect("groups");
    assert_eq!(groups[0]["rows"].as_array().expect("members").len(), 2);
    assert_eq!(groups[1]["rows"].as_array().expect("members").len(), 1);
}

#[test]
fn aliases_on_prebuilt_groups_reject_existing_summary_names() {
    let groups = OutputEnvelope::groups(
        vec![json!({
            "groups": {"Rack": "a"},
            "aggregates": {"Hosts": 2},
            "rows": [{"name": "one"}, {"name": "two"}]
        })],
        vec!["Rack".to_string(), "Hosts".to_string()],
    );

    for name in ["Rack", "Hosts"] {
        let error = apply_pipeline(
            groups.clone(),
            &[PipeStage::Columns(vec![alias("missing", name)])],
        )
        .expect_err("existing group output name must be protected");
        assert!(error.to_string().contains(name));
    }
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

#[test]
fn multi_key_sorting_is_lexicographic_and_stable() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"id": "one", "state": "b", "rank": 1}),
            json!({"id": "two", "state": "a", "rank": 1}),
            json!({"id": "three", "state": "a", "rank": 2}),
            json!({"id": "four", "state": "a", "rank": 2}),
        ],
        vec!["id".to_string(), "state".to_string(), "rank".to_string()],
    );

    let sorted = apply_dsl(rows, "S state asc, rank desc AS num").expect("multi-key sort");

    assert_eq!(output_ids(&sorted), ["three", "four", "two", "one"]);
}

#[test]
fn nulls_default_last_independently_of_direction_and_can_be_overridden() {
    let rows = || {
        OutputEnvelope::rows(
            vec![
                json!({"id": "missing"}),
                json!({"id": "null", "score": null}),
                json!({"id": "one", "score": 1}),
                json!({"id": "two", "score": 2}),
            ],
            vec!["id".to_string(), "score".to_string()],
        )
    };

    let default = apply_dsl(rows(), "S score desc AS num").expect("default null ordering");
    assert_eq!(output_ids(&default), ["two", "one", "missing", "null"]);

    let first = apply_dsl(rows(), "S score desc AS num NULLS FIRST").expect("nulls first");
    assert_eq!(output_ids(&first), ["missing", "null", "two", "one"]);
}

#[test]
fn fanout_sorting_supports_first_min_and_max_reduction() {
    let rows = || {
        OutputEnvelope::rows(
            vec![
                json!({"id": "wide", "scores": [9, 1]}),
                json!({"id": "middle", "scores": [5]}),
            ],
            vec!["id".to_string(), "scores".to_string()],
        )
    };

    let first = apply_dsl(rows(), "S scores[] AS num").expect("first reduction");
    assert_eq!(output_ids(&first), ["middle", "wide"]);
    let minimum = apply_dsl(rows(), "S scores[] AS num USING min").expect("min reduction");
    assert_eq!(output_ids(&minimum), ["wide", "middle"]);
    let maximum = apply_dsl(rows(), "S scores[] AS num USING max").expect("max reduction");
    assert_eq!(output_ids(&maximum), ["middle", "wide"]);
}

#[test]
fn sorting_uses_the_shared_strict_cast_policy() {
    for (source, values, expected) in [
        (
            "S value AS str",
            vec![
                json!({"id": "two", "value": 2}),
                json!({"id": "ten", "value": 10}),
            ],
            ["ten", "two"],
        ),
        (
            "S value AS num",
            vec![
                json!({"id": "ten", "value": "10"}),
                json!({"id": "two", "value": "2"}),
            ],
            ["two", "ten"],
        ),
        (
            "S value AS bool",
            vec![
                json!({"id": "true", "value": "TRUE"}),
                json!({"id": "false", "value": "false"}),
            ],
            ["false", "true"],
        ),
        (
            "S value AS datetime",
            vec![
                json!({"id": "later", "value": "2025-12-31T23:00:00Z"}),
                json!({"id": "earlier", "value": "2026-01-01T00:00:00+02:00"}),
            ],
            ["earlier", "later"],
        ),
        (
            "S value AS version",
            vec![
                json!({"id": "ten", "value": "10.0.0"}),
                json!({"id": "two", "value": "2.0.0"}),
            ],
            ["two", "ten"],
        ),
        (
            "S value AS natural",
            vec![
                json!({"id": "ten", "value": "host10"}),
                json!({"id": "two", "value": "host2"}),
            ],
            ["two", "ten"],
        ),
    ] {
        let rows = OutputEnvelope::rows(values, vec!["id".to_string(), "value".to_string()]);
        let sorted = apply_dsl(rows, source).expect(source);
        assert_eq!(output_ids(&sorted), expected, "{source}");
    }
}

#[test]
fn sort_cast_errors_identify_key_selector_row_and_value() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"id": "one", "group": "a", "scores": [1, 2]}),
            json!({"id": "two", "group": "a", "scores": [3, "bad"]}),
        ],
        vec!["id".to_string()],
    );

    let error = apply_dsl(rows, "S group, scores[] AS num USING min")
        .expect_err("every fanout value must cast");
    let message = error.to_string();
    assert!(message.contains("sort key 2"), "{message}");
    assert!(message.contains("scores[]"), "{message}");
    assert!(message.contains("row 2"), "{message}");
    assert!(message.contains("\"bad\""), "{message}");
}

#[test]
fn strict_ip_sorting_orders_numeric_addresses_and_keeps_families_distinct() {
    let rows = || {
        OutputEnvelope::rows(
            vec![
                json!({"id": "v6-ten", "address": "2001:db8::10"}),
                json!({"id": "v4-ten", "address": "10.0.0.10"}),
                json!({"id": "mapped", "address": "::ffff:192.0.2.1"}),
                json!({"id": "v6-two", "address": "2001:db8::2"}),
                json!({"id": "v4-two", "address": "10.0.0.2"}),
            ],
            vec!["id".to_string(), "address".to_string()],
        )
    };

    let ascending = apply_dsl(rows(), "S address AS ip").expect("ascending IP sort");
    assert_eq!(
        output_ids(&ascending),
        ["v4-two", "v4-ten", "mapped", "v6-two", "v6-ten"]
    );

    let descending = apply_dsl(rows(), "S address desc AS ip").expect("descending IP sort");
    assert_eq!(
        output_ids(&descending),
        ["v6-ten", "v6-two", "mapped", "v4-ten", "v4-two"]
    );
}

#[test]
fn ip_sorting_composes_with_multi_key_fanout_and_null_placement() {
    let rows = || {
        OutputEnvelope::rows(
            vec![
                json!({"id": "missing", "rack": "a"}),
                json!({"id": "edge", "rack": "a", "addresses": ["2001:db8::2", "10.0.0.9"]}),
                json!({"id": "core", "rack": "a", "addresses": ["10.0.0.10"]}),
            ],
            vec!["id".to_string(), "rack".to_string()],
        )
    };

    let minimum = apply_dsl(rows(), "S rack, addresses[] AS ip USING min NULLS FIRST")
        .expect("minimum IP fanout sort");
    assert_eq!(output_ids(&minimum), ["missing", "edge", "core"]);

    let maximum = apply_dsl(rows(), "S rack, addresses[] AS ip USING max NULLS FIRST")
        .expect("maximum IP fanout sort");
    assert_eq!(output_ids(&maximum), ["missing", "core", "edge"]);
}

#[test]
fn invalid_ip_sort_values_fail_with_full_context() {
    let rows = OutputEnvelope::rows(
        vec![
            json!({"id": "valid", "address": "192.0.2.1"}),
            json!({"id": "invalid", "address": "192.0.2.999"}),
        ],
        vec!["id".to_string(), "address".to_string()],
    );

    let error = apply_dsl(rows, "S address AS ip").expect_err("invalid IP must fail");
    let message = error.to_string();
    assert!(message.contains("sort key 1"), "{message}");
    assert!(message.contains("address"), "{message}");
    assert!(message.contains("row 2"), "{message}");
    assert!(message.contains("192.0.2.999"), "{message}");
}
