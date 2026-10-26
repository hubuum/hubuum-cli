use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

use super::*;
use crate::diagnostics;

fn inventory() -> Value {
    json!({"sampled": true, "sampled_objects": 2, "effective_sample_limit": 100,
    "max_depth": 6, "include_containers": false, "truncation": ["sample_limit"],
    "fields": [
        {"field": "data.a.b", "source": "data", "path": [{"kind":"property", "name":"a.b"}],
            "observation_count": 2, "object_count": 2, "types": {"string":2}, "example":"literal", "example_truncated":false, "error_count":0},
        {"field": "data.a.b", "source": "data", "path": [{"kind":"property", "name":"a"}, {"kind":"property", "name":"b"}],
            "observation_count": 3, "object_count": 2, "types": {"null":1,"number":2}, "example":"7", "example_truncated":true, "error_count":0},
        {"field": "P:note", "source": "personal_computed", "path": [{"kind":"property", "name":"note"}],
            "observation_count": 0, "object_count": 0, "types": {}, "example":null, "example_truncated":false, "error_count":2}
    ]})
}

#[tokio::test]
#[serial]
async fn field_discovery_uses_one_bounded_request_for_both_command_names() {
    for command in ["class fields --name Hosts", "object fields --class Hosts"] {
        let transport = MockTransport::default();
        respond(&transport, inventory(), None);
        let (result, trace) = diagnostics::capture_command(execute(
            &transport,
            &format!("{command} --debug --output json"),
        ))
        .with_subscriber(tracing_subscriber::registry().with(diagnostics::layer()))
        .await;
        let result = result.unwrap();
        assert!(result
            .output
            .warnings
            .iter()
            .any(|warning| warning.contains("sample_limit")));
        let rows: Value = from_str(&result.output.render()).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 3);
        assert_ne!(rows[0]["Path"], rows[1]["Path"]);
        assert_eq!(rows[1]["Count"], 3);
        assert_eq!(rows[1]["Objects"], 2);
        assert_eq!(rows[1]["TypeCounts"], json!({"null":1,"number":2}));
        assert_eq!(rows[2]["Errors"], 2);
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url.path(),
            "/api/v1/classes/by-name/Hosts/fields"
        );
        let pairs = requests[0].url.query_pairs().collect::<Vec<_>>();
        assert!(pairs.iter().any(|(k, v)| k == "limit" && v == "100"));
        assert!(pairs.iter().any(|(k, v)| k == "depth" && v == "6"));
        assert!(pairs.iter().any(|(k, v)| k == "containers" && v == "false"));
        let report = serde_json::to_value(trace.unwrap().snapshot().unwrap()).unwrap();
        assert_eq!(report["client_api_calls"], 1);
        assert!(report["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["name"] == "prepare_field_output"));
    }
}

#[tokio::test]
#[serial]
async fn field_discovery_preserves_pipeline_columns_and_server_sample_bounds() {
    let transport = MockTransport::default();
    let mut response = inventory();
    response["effective_sample_limit"] = json!(250);
    response["max_depth"] = json!(0);
    response["include_containers"] = json!(true);
    respond(&transport, response, None);
    let outcome = execute(
        &transport,
        "class fields Hosts --limit 500 --depth 0 --containers --output json | P Field Count Limit",
    )
    .await
    .unwrap();
    let result: Value = from_str(&outcome.output.render()).unwrap();
    assert_eq!(
        result[1],
        json!({"Field":"data.a.b", "Count":3, "Limit":250})
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let pairs = requests[0].url.query_pairs().collect::<Vec<_>>();
    assert!(pairs.iter().any(|(k, v)| k == "limit" && v == "250"));
    assert!(pairs.iter().any(|(k, v)| k == "depth" && v == "0"));
    assert!(pairs.iter().any(|(k, v)| k == "containers" && v == "true"));
}

#[tokio::test]
#[serial]
async fn invalid_discovery_bounds_fail_before_fetching() {
    let transport = MockTransport::default();
    for command in [
        "class fields Hosts --limit 0",
        "class fields Hosts --depth 33",
        "class fields --name ' '",
    ] {
        assert!(execute(&transport, command).await.is_err());
        assert!(transport.requests().is_empty());
    }
}
