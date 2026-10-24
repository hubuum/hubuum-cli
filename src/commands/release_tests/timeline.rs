use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

use super::*;
use crate::diagnostics;

fn version(revision: u32, memory: u32) -> Value {
    let mut record = object_history(2);
    record["history_id"] = json!(90000 + revision);
    record["revision"] = json!(revision);
    record["op"] = json!(if revision == 1 { "insert" } else { "update" });
    record["data"] = json!({"memory": memory});
    record
}

fn marker(record: &Value) -> Value {
    let mut marker = record.clone();
    for field in [
        "data",
        "json_schema",
        "description",
        "created_at",
        "updated_at",
        "validate_schema",
    ] {
        marker.as_object_mut().unwrap().remove(field);
    }
    marker
}

#[tokio::test]
#[serial]
async fn previous_comparison_fetches_the_boundary_and_batches_all_snapshots() {
    for direction in ["older", "newer"] {
        let transport = MockTransport::default();
        let (old, middle, new) = (version(1, 10), version(2, 20), version(3, 30));
        let ordered = if direction == "older" {
            vec![marker(&new), marker(&middle)]
        } else {
            vec![marker(&middle), marker(&new)]
        };
        respond(&transport, json!(ordered), Some("display-next"));
        respond(&transport, json!([marker(&old)]), None);
        // A continuation is deliberately present: all needed snapshots are in this batch.
        respond(
            &transport,
            json!([new, middle, old]),
            Some("unused-snapshot-page"),
        );
        let result = output(execute(&transport, &format!("history timeline object --class-id 1 --object-id 2 --from 2026-10-01 --until 2026-11 --direction {direction} --output json")).await.unwrap());
        assert_eq!(result["next_cursor"], "display-next");
        assert_eq!(result["returned_count"], 2);
        for row in result["items"].as_array().unwrap() {
            let revision = row["revision"].as_u64().unwrap();
            assert_eq!(row["changes"]["mode"], "previous");
            assert_eq!(row["changes"]["against_history_id"], 90000 + revision - 1);
            assert_eq!(
                row["changes"]["summary"],
                format!("/data/memory: {} → {}", (revision - 1) * 10, revision * 10)
            );
        }
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].url.query_pairs().any(|(k, _)| k == "from"));
        let reference = requests[1].url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(reference["anchor"], "90002");
        assert_eq!(reference["direction"], "older");
        assert_eq!(reference["limit"], "1");
        assert!(!reference.contains_key("from"));
        assert!(!reference.contains_key("until"));
        assert!(!reference.contains_key("cursor"));
        assert_eq!(requests[2].url.path(), "/api/v1/classes/1/2/history");
        let batch = requests[2].url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(batch["limit"], "250");
        assert_eq!(batch["sort"], "history_id.desc");
    }
}

#[tokio::test]
#[serial]
async fn latest_comparison_uses_a_reference_outside_the_selected_window_and_pages_snapshots() {
    let transport = MockTransport::default();
    let (old, new, latest) = (version(1, 10), version(2, 20), version(3, 30));
    respond(&transport, json!([marker(&new), marker(&old)]), None);
    respond(&transport, json!([marker(&latest)]), None);
    respond(&transport, json!([latest, new]), Some("snapshots-next"));
    respond(&transport, json!([old]), None);
    let result = output(execute(&transport, "history timeline object --class-id 1 --object-id 2 --until 2026-11 --anchor 90003 --against-latest --output json | JQ '.'").await.unwrap());
    for row in result["items"].as_array().unwrap() {
        assert_eq!(row["changes"]["mode"], "latest");
        assert_eq!(row["changes"]["against_history_id"], 90003);
    }
    assert_eq!(
        result["items"][0]["changes"]["summary"],
        "/data/memory: 20 → 30"
    );
    assert_eq!(
        result["items"][1]["changes"]["summary"],
        "/data/memory: 10 → 30"
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 4);
    let latest = requests[1].url.query_pairs().collect::<HashMap<_, _>>();
    for name in ["until", "anchor", "at", "cursor"] {
        assert!(!latest.contains_key(name));
    }
    assert!(requests[3]
        .url
        .query_pairs()
        .any(|(k, v)| k == "cursor" && v == "snapshots-next"));
    assert!(requests
        .iter()
        .all(|request| !request.url.path().contains("/entries/")));
}

#[tokio::test]
#[serial]
async fn class_timeline_displays_changes_in_a_compact_table() {
    let transport = MockTransport::default();
    let mut old = class_history();
    old["op"] = json!("insert");
    old["revision"] = json!(1);
    old["history_id"] = json!(90001);
    let mut new = old.clone();
    new["op"] = json!("update");
    new["revision"] = json!(2);
    new["history_id"] = json!(90002);
    new["description"] = json!("new");
    respond(&transport, json!([marker(&new)]), Some("next"));
    respond(&transport, json!([marker(&old)]), None);
    respond(&transport, json!([new, old]), None);
    let result = execute(&transport, "history timeline class --class-id 1 --limit 1")
        .await
        .unwrap();
    let text = result.output.render();
    assert!(text.contains("changes"), "{text}");
    assert!(!text.contains("Changes for history"), "{text}");
    assert_eq!(
        result.output.semantic[0].value()[0]["changes"],
        "/description: \"old\" → \"new\""
    );
    assert!(transport
        .requests()
        .iter()
        .all(|r| r.url.path().starts_with("/api/v1/classes/1/history")));
}

#[tokio::test]
#[serial]
async fn missing_snapshots_are_explicit_and_snapshot_cursor_cycles_fail() {
    let transport = MockTransport::default();
    let record = version(2, 20);
    respond(&transport, json!([marker(&record)]), None);
    respond(&transport, json!([]), None);
    respond(&transport, json!([]), None);
    let result = output(
        execute(
            &transport,
            "history timeline object --class-id 1 --object-id 2 --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        result["items"][0]["changes"]["summary"],
        "Snapshot unavailable"
    );

    let transport = MockTransport::default();
    respond(&transport, json!([marker(&record)]), None);
    respond(&transport, json!([]), None);
    respond(&transport, json!([version(4, 40)]), Some("private-cycle"));
    respond(&transport, json!([version(3, 30)]), Some("private-cycle"));
    let error = execute(
        &transport,
        "history timeline object --class-id 1 --object-id 2",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("repeated cursor"), "{error}");
    assert!(!error.contains("private-cycle"));
}

#[tokio::test]
#[serial]
async fn empty_timelines_do_not_fetch_comparison_data() {
    for flag in ["", "--against-latest"] {
        let transport = MockTransport::default();
        respond(&transport, json!([]), None);
        let result = output(
            execute(
                &transport,
                &format!("history timeline object --class-id 1 --object-id 2 {flag} --output json"),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["items"], json!([]));
        assert_eq!(transport.requests().len(), 1);
    }
}

#[tokio::test]
#[serial]
async fn boundary_lookup_follows_empty_pages_and_missing_baselines_are_not_creation() {
    let transport = MockTransport::default();
    let current = version(5, 50);
    let previous = version(2, 20);
    respond(&transport, json!([marker(&current)]), None);
    respond(&transport, json!([]), Some("reference-next"));
    respond(&transport, json!([marker(&previous)]), None);
    respond(&transport, json!([current]), None);
    let result = output(
        execute(
            &transport,
            "history timeline object --class-id 1 --object-id 2 --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(result["items"][0]["changes"]["against_history_id"], 90002);
    assert_eq!(
        result["items"][0]["changes"]["summary"],
        "Previous retained entry unavailable"
    );
    assert!(transport.requests()[2]
        .url
        .query_pairs()
        .any(|(k, v)| k == "cursor" && v == "reference-next"));
}

#[tokio::test]
#[serial]
async fn snapshot_authorization_errors_fail_the_command() {
    let transport = MockTransport::default();
    respond(&transport, json!([marker(&version(2, 20))]), None);
    respond(&transport, json!([]), None);
    transport.push_response(TransportResponse::empty(StatusCode::FORBIDDEN));
    assert!(execute(
        &transport,
        "history timeline object --class-id 1 --object-id 2 --output json"
    )
    .await
    .is_err());
    assert_eq!(transport.requests().len(), 3);
}

#[tokio::test]
#[serial]
async fn initial_creation_skips_the_previous_lookup_and_snapshot_pages_are_counted() {
    for against_latest in [false, true] {
        let transport = MockTransport::default();
        let first = version(1, 10);
        let latest = version(2, 20);
        respond(&transport, json!([marker(&latest), marker(&first)]), None);
        if against_latest {
            respond(&transport, json!([marker(&latest)]), None);
        }
        respond(&transport, json!([latest, first]), None);
        let flag = if against_latest {
            "--against-latest"
        } else {
            ""
        };
        let command = format!(
            "history timeline object --class-id 1 --object-id 2 {flag} --debug --output json"
        );
        let (result, trace) = diagnostics::capture_command(execute(&transport, &command))
            .with_subscriber(tracing_subscriber::registry().with(diagnostics::layer()))
            .await;
        let result = output(result.unwrap());
        if against_latest {
            assert_eq!(result["items"][0]["changes"]["against_history_id"], 90002);
        } else {
            assert!(result["items"][1]["changes"]["against_history_id"].is_null());
            assert!(result["items"][1]["changes"]["summary"]
                .as_str()
                .unwrap()
                .starts_with("Created\n"));
        }
        let expected = if against_latest { 3 } else { 2 };
        assert_eq!(transport.requests().len(), expected);
        let report = serde_json::to_value(trace.unwrap().snapshot().unwrap()).unwrap();
        assert_eq!(report["client_api_calls"], expected);
        assert_eq!(report["custom_transport_attempts"], expected);
        assert!(report["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["name"] == "format_output"));
    }
}
