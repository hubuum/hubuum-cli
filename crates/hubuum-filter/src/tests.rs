use crate::{
    apply_pipeline, group_summary_rows, split_pipeline, AggregateFunction, OutputEnvelope,
    OutputShape, PipeStage, ProjectTerm, SortCast,
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
fn grouping_aggregates_and_count_use_host_examples() {
    let grouped = apply_pipeline(
        host_rows(),
        &[
            PipeStage::Group(vec![crate::GroupKey {
                selector: "os_version".to_string(),
                alias: "OS Version".to_string(),
            }]),
            PipeStage::Aggregate(crate::AggregateSpec {
                function: AggregateFunction::Count,
                alias: "Hosts".to_string(),
            }),
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
            PipeStage::Group(vec![crate::GroupKey {
                selector: "os_version".to_string(),
                alias: "OS Version".to_string(),
            }]),
            PipeStage::Aggregate(crate::AggregateSpec {
                function: AggregateFunction::Count,
                alias: "Hosts".to_string(),
            }),
            PipeStage::SortColumn {
                column: "Hosts".to_string(),
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
fn grouping_fans_out_array_selectors() {
    let grouped = apply_pipeline(
        host_rows(),
        &[PipeStage::Group(vec![crate::GroupKey {
            selector: "data.network.interfaces[].ipv4".to_string(),
            alias: "IPv4".to_string(),
        }])],
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
            ProjectTerm::keep("Name"),
            ProjectTerm::keep("data"),
            ProjectTerm::drop("data.cpu"),
        ])],
    )
    .expect("project");
    assert!(projected.value.to_string().contains("network"));
    assert!(!projected.value.to_string().contains("cores"));

    let values = apply_pipeline(
        host_rows(),
        &[PipeStage::Value(
            "data.network.interfaces[-1].ipv4".to_string(),
        )],
    )
    .expect("value");
    assert_eq!(values.value, json!(["10.0.0.10", "129.240.1.11"]));
}

#[test]
fn parsing_rejects_unknown_single_letter_stages() {
    assert!(split_pipeline("object list --class Hosts | X foo").is_err());
    assert!(split_pipeline("object list --class Hosts | owner").is_ok());
}
