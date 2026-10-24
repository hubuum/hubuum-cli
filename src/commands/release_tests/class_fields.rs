use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

use super::*;
use crate::diagnostics;

fn definition(key: &str, visibility: &str, enabled: bool) -> Value {
    json!({
        "id": 1, "class_id": 9, "visibility": visibility, "key": key,
        "label": key, "description": "", "enabled": enabled,
        "operation": {"type": "sum", "paths": ["/load"]}, "result_type": "number",
        "revision": 1, "semantics_version": 1, "created_at": AT, "updated_at": AT
    })
}

fn shared_definitions(definitions: Value) -> Value {
    json!({
        "definitions": definitions,
        "state": {"class_id": 9, "evaluation_revision": 1, "rebuild_status": "idle",
            "created_at": AT, "updated_at": AT}
    })
}

fn sample_object() -> Value {
    let fixture: Value =
        from_str(include_str!("../../../tests/fixtures/object-show.json")).unwrap();
    let mut object = fixture["graph"]["objects"][0].clone();
    object["collection_id"] = json!(999);
    object["data"] = json!({"hardware": {"cpu": "M2"}, "nullable": null});
    object["computed"] = json!({
        "shared": {"revision": 1, "materialization_stale": false,
            "values": {"load": 1.5, "disabled": 10, "unknown": 20}, "errors": {}},
        "personal": {"values": {"note": "mine", "nullable": null},
            "errors": {"missing": {"code": "invalid", "message": "missing input"}}}
    });
    object
}

fn respond_class(transport: &MockTransport) {
    let fixture: Value =
        from_str(include_str!("../../../tests/fixtures/object-show.json")).unwrap();
    respond(transport, fixture["root_class"].clone(), None);
}

fn field_rows(outcome: CommandOutcome) -> Value {
    // Warnings (deprecated alias or clamped limit) are rendered separately from the rows.
    from_str(&outcome.output.lines.join("\n")).unwrap()
}

#[tokio::test]
#[serial]
async fn field_discovery_preserves_output_with_four_requests_and_local_timings() {
    for command in ["class fields --name Hosts", "object fields --class Hosts"] {
        for enabled in [false, true] {
            let transport = MockTransport::default();
            respond_class(&transport);
            respond(
                &transport,
                shared_definitions(json!([
                    definition("load", "shared", enabled),
                    definition("disabled", "shared", false)
                ])),
                None,
            );
            respond(
                &transport,
                json!([
                    definition("note", "personal", enabled),
                    definition("nullable", "personal", enabled),
                    definition("missing", "personal", enabled)
                ]),
                None,
            );
            // A continuation must not cause discovery to exceed the sample size.
            respond(
                &transport,
                json!([sample_object()]),
                Some("unused-object-page"),
            );
            let command = format!("{command} --limit 1 --debug --output json");
            let (result, trace) = diagnostics::capture_command(execute(&transport, &command))
                .with_subscriber(tracing_subscriber::registry().with(diagnostics::layer()))
                .await;
            let result = field_rows(result.unwrap());
            let mut expected = vec![
                json!({"Field": "data.hardware.cpu", "Source": "data", "Count": 1,
                    "Types": "string", "Example": "M2", "Sample": 1, "Limit": 1}),
                json!({"Field": "data.nullable", "Source": "data", "Count": 1,
                    "Types": "null", "Example": "null", "Sample": 1, "Limit": 1}),
            ];
            if enabled {
                expected.extend([
                    json!({"Field": "S:load", "Source": "shared computed", "Count": 1,
                        "Types": "number", "Example": "1.5", "Sample": 1, "Limit": 1}),
                    json!({"Field": "P:missing", "Source": "personal computed", "Count": 0,
                        "Types": "", "Example": "", "Sample": 1, "Limit": 1}),
                    json!({"Field": "P:note", "Source": "personal computed", "Count": 1,
                        "Types": "string", "Example": "mine", "Sample": 1, "Limit": 1}),
                    json!({"Field": "P:nullable", "Source": "personal computed", "Count": 1,
                        "Types": "null", "Example": "null", "Sample": 1, "Limit": 1}),
                ]);
            }
            assert_eq!(result, json!(expected));
            let requests = transport.requests();
            assert_eq!(
                requests
                    .iter()
                    .map(|request| request.url.path())
                    .collect::<Vec<_>>(),
                [
                    "/api/v1/classes/by-name/Hosts",
                    "/api/v1/classes/9/computed-fields",
                    "/api/v1/iam/me/computed-fields",
                    "/api/v1/classes/9/",
                ]
            );
            assert!(requests[2]
                .url
                .query_pairs()
                .any(|(k, v)| k == "class_id" && v == "9"));
            let pairs = requests[3].url.query_pairs().collect::<Vec<_>>();
            assert!(pairs.iter().any(|(k, v)| k == "limit" && v == "1"));
            assert!(pairs
                .iter()
                .any(|(k, v)| k == "include_total" && v == "false"));
            assert_eq!(
                pairs.iter().any(|(k, v)| k == "include" && v == "computed"),
                enabled
            );
            let report = serde_json::to_value(trace.unwrap().snapshot().unwrap()).unwrap();
            assert_eq!(report["client_api_calls"], 4);
            assert_eq!(report["custom_transport_attempts"], 4);
            for stage in [
                "prepare_field_sample",
                "summarize_fields",
                "prepare_field_output",
            ] {
                assert!(report["spans"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|span| span["name"] == stage));
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn field_discovery_reads_all_personal_definitions_before_sampling() {
    let transport = MockTransport::default();
    respond_class(&transport);
    respond(&transport, shared_definitions(json!([])), None);
    respond(
        &transport,
        json!([definition("disabled", "personal", false)]),
        Some("personal-next"),
    );
    respond(
        &transport,
        json!([definition("note", "personal", true)]),
        None,
    );
    respond(
        &transport,
        json!([sample_object()]),
        Some("unused-object-page"),
    );
    let result = field_rows(execute(&transport,
        "class fields Hosts --limit 500 --depth 1 --containers --output json | P Field Count Limit"
    ).await.unwrap());
    assert_eq!(
        result,
        json!([
            {"Field": "data.hardware", "Count": 1, "Limit": 250},
            {"Field": "data.nullable", "Count": 1, "Limit": 250},
            {"Field": "P:note", "Count": 1, "Limit": 250},
        ])
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 5);
    for request in &requests[2..4] {
        assert_eq!(request.url.path(), "/api/v1/iam/me/computed-fields");
        assert!(request
            .url
            .query_pairs()
            .any(|(k, v)| k == "class_id" && v == "9"));
    }
    assert!(requests[3]
        .url
        .query_pairs()
        .any(|(k, v)| k == "cursor" && v == "personal-next"));
    assert!(requests[4]
        .url
        .query_pairs()
        .any(|(k, v)| k == "include" && v == "computed"));
    assert!(requests[4]
        .url
        .query_pairs()
        .any(|(k, v)| k == "limit" && v == "250"));
}

#[tokio::test]
#[serial]
async fn empty_field_samples_keep_enabled_definitions_and_validate_before_fetching() {
    let transport = MockTransport::default();
    for command in ["class fields Hosts --limit 0", "class fields --name ' '"] {
        assert!(execute(&transport, command).await.is_err());
        assert!(transport.requests().is_empty());
    }
    respond_class(&transport);
    respond(
        &transport,
        shared_definitions(json!([definition("load", "shared", true)])),
        None,
    );
    respond(&transport, json!([]), None);
    respond(&transport, json!([]), None);
    let result = output(
        execute(&transport, "class fields Hosts --output json")
            .await
            .unwrap(),
    );
    assert_eq!(
        result,
        json!([
            {"Field": "S:load", "Source": "shared computed", "Count": 0,
                "Types": "", "Example": "", "Sample": 0, "Limit": 100}
        ])
    );
    assert_eq!(transport.requests().len(), 4);
}
