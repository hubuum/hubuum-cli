use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
use reqwest::{header::HeaderValue, Method, StatusCode};
use serde_json::{from_str, json, Value};
use serial_test::serial;

use crate::app::AppRuntime;
use crate::catalog::{CatalogStore, CommandOutcome};
use crate::config::{get_config, init_config, AppConfig};
use crate::dispatch::execute_line;
use crate::errors::AppError;
use crate::repl::SharedReplState;
use crate::services::AppServices;

const AT: &str = "2026-10-23T00:00:00Z";

mod timeline;

async fn execute(transport: &MockTransport, command: &str) -> Result<CommandOutcome, AppError> {
    let previous = get_config();
    let config = Arc::new(AppConfig::default());
    init_config(config.clone()).unwrap();
    let transport = transport.clone();
    let client = tokio::task::spawn_blocking(move || {
        Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport))
            .build()
            .unwrap()
            .authenticate(Token::new("secret"))
    })
    .await
    .unwrap();
    let services = Arc::new(AppServices::new(
        Arc::new(client),
        tokio::runtime::Handle::current(),
        Duration::from_secs(60),
    ));
    let catalog = Arc::new(CatalogStore::new(super::build_command_catalog()));
    let app = Arc::new(AppRuntime::new(config, services, catalog));
    let result = execute_line(app.clone(), &SharedReplState::new(), command).await;
    tokio::task::spawn_blocking(move || drop(app))
        .await
        .unwrap();
    init_config(previous).unwrap();
    result
}

fn respond(transport: &MockTransport, value: Value, next: Option<&str>) {
    let mut response = TransportResponse::json(StatusCode::OK, &value).unwrap();
    if let Some(next) = next {
        response
            .headers
            .insert("x-next-cursor", HeaderValue::from_str(next).unwrap());
    }
    response
        .headers
        .insert("x-page-limit", HeaderValue::from_static("1"));
    transport.push_response(response);
}

fn class_history() -> Value {
    json!({"id":1,"name":"deleted class","collection_id":1,"validate_schema":true,"description":"old",
        "json_schema":{"type":"object"},"created_at":AT,"updated_at":AT,"revision":2,
        "history_id":90001,"op":"delete","valid_from":AT,"valid_to":null})
}

fn object_history(id: i32) -> Value {
    let mut value = class_history();
    value["id"] = json!(id);
    value["name"] = json!("123");
    value["hubuum_class_id"] = json!(1);
    value["data"] = json!({"retained":"secret snapshot"});
    value
}

fn output(outcome: CommandOutcome) -> Value {
    from_str(&outcome.output.render()).unwrap()
}

#[tokio::test]
#[serial]
async fn task_output_pipelines_read_the_complete_response() {
    let transport = MockTransport::default();
    respond(
        &transport,
        json!({
            "id":5,"kind":"import","status":"succeeded","created_at":AT,
            "progress":{"total_items":0,"processed_items":0,"success_items":0,"failed_items":0},
            "links":{"task":"/api/v1/tasks/5","events":"/api/v1/tasks/5/events"}
        }),
        None,
    );
    respond(&transport, json!([]), None);
    let result = execute(
        &transport,
        "task output 5 --output json | JQ '.ImportResults | length'",
    )
    .await
    .unwrap();
    assert_eq!(output(result), json!(0));
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test]
#[serial]
async fn deletion_confirmations_support_message_extraction() {
    let transport = MockTransport::default();
    let fixture: Value = from_str(include_str!("../../tests/fixtures/object-show.json")).unwrap();
    respond(&transport, fixture["root_class"].clone(), None);
    transport.push_response(TransportResponse::empty(StatusCode::NO_CONTENT));
    let result = execute(&transport, "class delete example | VALUE message")
        .await
        .unwrap();
    assert_eq!(
        result.output.render(),
        "Class 'example' deleted successfully\n"
    );
    assert_eq!(transport.requests().len(), 2);
    assert_eq!(transport.requests()[1].method, Method::DELETE);
}

#[tokio::test]
#[serial]
async fn history_ids_fetch_deleted_entries_directly_and_preserve_snapshots() {
    for (command, path, response) in [
        (
            "history show --class-id 1 --id 90001 --output json",
            "/api/v1/classes/1/history/entries/90001",
            class_history(),
        ),
        (
            "history show --class-id 1 --object-id 2 --id 90001 --output json",
            "/api/v1/classes/1/2/history/entries/90001",
            object_history(2),
        ),
    ] {
        let transport = MockTransport::default();
        respond(&transport, response, None);
        let result = output(execute(&transport, command).await.unwrap());
        assert_eq!(result["history_id"], 90001);
        assert_eq!(result["op"], "delete");
        assert_eq!(transport.requests().len(), 1);
        assert_eq!(transport.requests()[0].url.path(), path);
    }
}

#[tokio::test]
#[serial]
async fn timelines_preserve_navigation_across_pages_and_reject_cursor_cycles() {
    let transport = MockTransport::default();
    respond(
        &transport,
        json!([object_history(2)]),
        Some("private-cursor"),
    );
    respond(&transport, json!([object_history(2)]), None);
    respond(&transport, json!([]), None);
    respond(&transport, json!([object_history(2)]), None);
    let result = output(execute(&transport, "history timeline object --class-id 1 --object-id 2 --anchor 42 --direction newer --from 2026-10-01T00:00:00Z --until 2026-11-01T00:00:00Z --all --output json").await.unwrap());
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["page_limit"], 1);
    assert!(result["next_cursor"].is_null());
    assert_eq!(transport.requests().len(), 4);
    for request in &transport.requests()[..2] {
        assert_eq!(request.url.path(), "/api/v1/classes/1/2/history/timeline");
        let query = request.url.query().unwrap();
        assert!(query.contains("anchor=42"));
        assert!(query.contains("direction=newer"));
        assert!(query.contains("from="));
        assert!(query.contains("until="));
    }
    let transport = MockTransport::default();
    respond(&transport, json!([]), Some("cycle-secret"));
    respond(&transport, json!([]), Some("cycle-secret"));
    let error = execute(&transport, "history timeline class --class-id 1 --all")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("repeated cursor"));
    assert!(!error.contains("cycle-secret"));
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test]
#[serial]
async fn historical_names_and_population_envelopes_survive_pagination_and_pipelines() {
    let transport = MockTransport::default();
    respond(&transport, json!([object_history(2)]), Some("next"));
    respond(&transport, json!([object_history(3)]), None);
    let result = output(
        execute(
            &transport,
            "history object-name --class-id 1 --name 123 --all --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(result["items"][0]["id"], 2);
    assert_eq!(result["items"][1]["id"], 3);
    for request in transport.requests() {
        assert_eq!(request.url.path(), "/api/v1/classes/1/objects/history");
        assert!(request
            .url
            .query_pairs()
            .any(|(key, value)| key == "name" && value == "123"));
    }
    let transport = MockTransport::default();
    for (id, next) in [(2, Some("next")), (3, None)] {
        respond(
            &transport,
            json!({"at":AT,"class":class_history(),"coverage":"retained_history","objects":[object_history(id)]}),
            next,
        );
    }
    let result = output(execute(&transport, "history objects --class-id 1 --at 2026-10-23T00:00:00Z --all --output json | JQ '{coverage, class, objects}'").await.unwrap());
    assert_eq!(result["coverage"], "retained_history");
    assert_eq!(result["class"]["json_schema"]["type"], "object");
    assert_eq!(result["objects"].as_array().unwrap().len(), 2);
}

#[tokio::test]
#[serial]
async fn relationship_and_computed_history_preserve_explicit_unavailability() {
    for (selector, suffix) in [("--class-id 1", "1"), ("--class-id 1 --object-id 2", "1/2")] {
        for at in ["", " --at 2026-10-23T00:00:00Z"] {
            let transport = MockTransport::default();
            respond(
                &transport,
                json!({"at":if at.is_empty() {Value::Null} else {json!(AT)},"coverage":"retained_history","relations":[]}),
                None,
            );
            let result = output(
                execute(
                    &transport,
                    &format!("history relations {selector}{at} --output json"),
                )
                .await
                .unwrap(),
            );
            assert_eq!(result["coverage"], "retained_history");
            let end = if at.is_empty() { "" } else { "/as-of" };
            assert_eq!(
                transport.requests()[0].url.path(),
                format!("/api/v1/classes/{suffix}/relations/history{end}")
            );
        }
    }
    let transport = MockTransport::default();
    respond(
        &transport,
        json!({"mode":"historical_recomputation","at":AT,"object":{"object_id":2,"class_id":1,"history_id":90001,"revision":2,"valid_from":AT,"valid_to":null},"shared":{"status":"unavailable","reason":"definition_history_unavailable"},"personal":"unsupported"}),
        None,
    );
    let result = output(
        execute(
            &transport,
            "history computed --class-id 1 --object-id 2 --at 2026-10-23T00:00:00Z --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(result["shared"]["reason"], "definition_history_unavailable");
    assert_eq!(result["personal"], "unsupported");
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
#[serial]
async fn structured_history_formats_keep_retention_context() {
    for format in ["jsonl", "csv", "tsv"] {
        let transport = MockTransport::default();
        respond(
            &transport,
            json!({"at":AT,"coverage":"retained_history","relations":[]}),
            None,
        );
        let result = execute(
            &transport,
            &format!("history relations --class-id 1 --output {format}"),
        )
        .await
        .unwrap();
        let page = result.output.semantic[0].value();
        assert_eq!(page["coverage"], "retained_history");
        assert_eq!(page["relations"], json!([]));
        assert!(!result.output.render().contains("Historical context:"));
    }
}

#[tokio::test]
#[serial]
async fn invalid_historical_and_hydration_options_fail_before_requests() {
    for command in [
        "history show --class Hosts --at 2026-02-29", "history show --class Hosts --at 1m1m",
        "history show --class-id -1 --id 42", "history object --class-id 1",
        "history show --class Hosts --class-id 1 --id 42",
        "history timeline class --class-id 1 --at 2026-10-23T00:00:00Z --anchor 42",
        "history timeline class --class-id 1 --from 2026-11-01T00:00:00Z --until 2026-10-01T00:00:00Z",
        "history timeline class --class-id 1 --limit 0",
        "history relations --class-id 1 --sort revision",
        "object hydrate --class-id 1 --object-id 2 --target-class Rooms --max-depth 11",
        "object hydrate --class-id 1 --object-id 2 --target-class Rooms --path-limit 0",
        "me token activity --token-id 1 --offset 257", "me token activity --token-id 1 --limit 101",
    ] {
        let transport = MockTransport::default();
        assert!(execute(&transport, command).await.is_err(), "{command}");
        assert!(transport.requests().is_empty(), "{command}");
    }
}

#[tokio::test]
#[serial]
async fn hydration_preserves_paths_and_reports_truncation_without_a_next_cursor() {
    let transport = MockTransport::default();
    respond(
        &transport,
        json!({"version":1,"root_object_id":2,"target_class_id":3,"depth":2,"limit":1,"objects":[],"relations":[],"paths":[{"object_ids":[2,3],"relation_ids":[4]}],"truncated":true}),
        None,
    );
    let result = execute(&transport, "object hydrate --class-id 1 --object-id 2 --target-class-id 3 --max-depth 2 --path-limit 1 --output json").await.unwrap();
    let rendered = result.output.render();
    assert!(rendered.contains("truncated"));
    assert!(rendered.contains("object_ids"));
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::POST);
    let body: Value = serde_json::from_slice(requests[0].body()).unwrap();
    assert_eq!(body["depth"], 2);
    assert_eq!(body["limit"], 1);
}

#[tokio::test]
#[serial]
async fn observation_controls_use_the_shared_override_and_correct_effects() {
    for (command, enabled, method) in [
        ("show", true, Method::GET),
        ("enable", true, Method::PUT),
        ("disable", false, Method::PUT),
    ] {
        let transport = MockTransport::default();
        respond(
            &transport,
            json!({"enabled":enabled,"runtime_override":enabled,"sample_every":16,"max_patterns":2048,"max_patterns_per_class":64,"retention_seconds":86400,"max_predicates_per_query":16}),
            None,
        );
        let result = output(
            execute(
                &transport,
                &format!("admin query-observations {command} --output json"),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["runtime_override"], enabled);
        let requests = transport.requests();
        assert_eq!(requests[0].method, method);
        if method == Method::PUT {
            let body: Value = serde_json::from_slice(requests[0].body()).unwrap();
            assert_eq!(body, json!({"enabled":enabled}));
        }
    }
}

fn service_account() -> Value {
    json!([{"id":7,"name":"collector","description":"","owner_group_id":2,"created_by":1,"disabled_at":null,"revision":1,"created_at":AT,"updated_at":AT}])
}

fn activity(offset: usize, next: Option<usize>) -> Value {
    json!({"session_id":"family","creation":{"issuer_id":1,"issuance_method":"creation","issued_at":AT,"address":null,"address_method":"unavailable"},
        "sources":[{"address":format!("192.0.2.{}",offset+1),"address_method":"connection_peer","first_seen":AT,"last_seen":AT}],
        "total":2,"offset":offset,"limit":1,"next_offset":next,"collection_enabled":false,"best_effort":true,
        "retention_days":30,"source_cap":256,"overflow_since":AT})
}

#[tokio::test]
#[serial]
async fn credential_activity_preserves_evidence_and_uses_bounded_offsets() {
    let transport = MockTransport::default();
    respond(&transport, service_account(), None);
    respond(&transport, activity(0, Some(1)), None);
    respond(&transport, activity(1, None), None);
    let value = output(execute(&transport, "service-account token activity --name collector --token-id 42 --limit 1 --all --output json | JQ '.'").await.unwrap());
    assert_eq!(value["sources"].as_array().unwrap().len(), 2);
    assert_eq!(value["creation"]["issuance_method"], "creation");
    assert_eq!(value["collection_enabled"], false);
    assert_eq!(value["best_effort"], true);
    assert_eq!(value["overflow_since"], "2026-10-23T00:00:00+00:00");
    assert!(value["next_offset"].is_null());
    let requests = transport.requests();
    assert_eq!(requests.len(), 3);
    for (offset, request) in requests[1..].iter().enumerate() {
        assert_eq!(
            request.url.path(),
            "/api/v1/iam/principals/7/tokens/42/activity"
        );
        assert!(request
            .url
            .query_pairs()
            .any(|(key, value)| key == "offset" && value == offset.to_string()));
    }
    for second in [activity(0, Some(1)), {
        let mut page = activity(1, None);
        page["collection_enabled"] = json!(true);
        page
    }] {
        let transport = MockTransport::default();
        respond(&transport, service_account(), None);
        respond(&transport, activity(0, Some(1)), None);
        respond(&transport, second, None);
        assert!(execute(
            &transport,
            "service-account token activity --name collector --token-id 42 --limit 1 --all"
        )
        .await
        .is_err());
        assert_eq!(transport.requests().len(), 3);
    }
}

#[tokio::test]
#[serial]
async fn history_listing_tables_have_revision_columns_and_json_keeps_data() {
    let transport = MockTransport::default();
    respond(&transport, json!([object_history(2)]), None);
    let result = execute(&transport, "history object --class-id 1 --object-id 2")
        .await
        .unwrap()
        .output
        .render();
    for field in [
        "history_id",
        "revision",
        "operation",
        "valid_from",
        "delete",
    ] {
        assert!(result.contains(field), "{result}");
    }
    let transport = MockTransport::default();
    respond(&transport, json!([object_history(2)]), None);
    let value = output(
        execute(
            &transport,
            "history object --class-id 1 --object-id 2 --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(value[0]["data"]["retained"], "secret snapshot");
}

#[tokio::test]
#[serial]
async fn history_prefix_fetches_every_marker_page_and_complete_matching_snapshot() {
    for command in ["history show", "history object"] {
        let transport = MockTransport::default();
        let mut first = object_history(2);
        first["history_id"] = json!(90002);
        first["revision"] = json!(3);
        first["op"] = json!("update");
        first["data"] = json!({"memory": 581});
        let mut second = object_history(2);
        second["op"] = json!("update");
        second["data"] = json!({"memory": 580});
        respond(&transport, json!([first]), Some("older"));
        respond(&transport, json!([second]), None);
        respond(&transport, json!([first, second]), None);
        let result = output(
            execute(
                &transport,
                &format!("{command} --class-id 1 --object-id 2 --at 2026-10 --output json"),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result.as_array().unwrap().len(), 2);
        assert_eq!(result[0]["data"], first["data"]);
        assert_eq!(result[1]["data"], second["data"]);
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        for request in &requests[..2] {
            assert_eq!(request.url.path(), "/api/v1/classes/1/2/history/timeline");
            let params = request.url.query_pairs().collect::<HashMap<_, _>>();
            assert_eq!(params["from"], "2026-10-01T00:00:00+00:00");
            assert_eq!(params["until"], "2026-11-01T00:00:00+00:00");
        }
        assert!(requests[1]
            .url
            .query_pairs()
            .any(|(key, value)| key == "cursor" && value == "older"));
        assert_eq!(requests[2].url.path(), "/api/v1/classes/1/2/history");
    }
}

#[tokio::test]
#[serial]
async fn history_prefix_without_changes_returns_an_empty_result() {
    let transport = MockTransport::default();
    respond(&transport, json!([]), None);
    let result = output(
        execute(
            &transport,
            "history class --class-id 1 --at 2026-10-23T15 --include-total --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(result["items"], json!([]));
    assert_eq!(result["total_count"], 0);
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
#[serial]
async fn exact_and_relative_history_times_use_as_of_endpoints() {
    for at in ["2026-10-23T00:00:00Z", "1d3h44m1s", "-1d3h44m1s"] {
        let transport = MockTransport::default();
        respond(&transport, object_history(2), None);
        let before = Utc::now() - ChronoDuration::seconds(99_841);
        let result = output(
            execute(
                &transport,
                &format!("history show --class-id 1 --object-id 2 --at {at} --output json"),
            )
            .await
            .unwrap(),
        );
        let after = Utc::now() - ChronoDuration::seconds(99_841);
        assert_eq!(result["history_id"], 90001);
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/api/v1/classes/1/2/history/as-of");
        let sent = requests[0]
            .url
            .query_pairs()
            .find(|(key, _)| key == "at")
            .unwrap()
            .1;
        let sent = DateTime::parse_from_rfc3339(&sent).unwrap();
        if at == AT {
            assert_eq!(sent, DateTime::parse_from_rfc3339(AT).unwrap());
        } else {
            assert!(sent >= before && sent <= after);
        }
    }
}

#[tokio::test]
#[serial]
async fn history_listing_displays_nested_field_changes() {
    let transport = MockTransport::default();
    let mut before = object_history(2);
    before["op"] = json!("update");
    before["data"] = json!({"memory": {"total": "580 GB"}});
    let mut after = before.clone();
    after["revision"] = json!(3);
    after["history_id"] = json!(90002);
    after["data"] = json!({"memory": {"total": "581 GB"}});
    respond(&transport, json!([after, before]), None);
    let result = execute(&transport, "history object --class-id 1 --object-id 2")
        .await
        .unwrap()
        .output
        .render();
    assert!(result.contains("Changes for history #90002"), "{result}");
    assert!(result.contains("/data/memory/total"), "{result}");
    assert!(result.contains("580 GB"), "{result}");
    assert!(result.contains("581 GB"), "{result}");
}

#[tokio::test]
#[serial]
async fn credential_activity_text_displays_creation_and_sources_as_nested_fields() {
    let transport = MockTransport::default();
    respond(&transport, service_account(), None);
    respond(&transport, activity(0, None), None);
    let result = execute(
        &transport,
        "service-account token activity --name collector --token-id 42",
    )
    .await
    .unwrap();
    let text = result.output.render();
    assert!(text.contains("creation:\n  address"), "{text}");
    assert!(text.contains("sources:\n  - address"), "{text}");
    assert!(!text.contains("{\""), "{text}");
    assert!(result.output.semantic[0].value()["sources"].is_array());
}

#[tokio::test]
#[serial]
async fn history_permission_failures_are_not_replaced_by_live_lookups() {
    let transport = MockTransport::default();
    transport.push_response(
        TransportResponse::json(StatusCode::FORBIDDEN, &json!({"error":"forbidden"})).unwrap(),
    );
    assert!(execute(
        &transport,
        "history show --class-id 1 --object-id 2 --id 42"
    )
    .await
    .is_err());
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
#[serial]
async fn credential_activity_accepts_a_positional_token_after_an_owner_option() {
    for command in [
        "service-account token activity --name collector 42",
        "service-account token activity collector 42",
        "user token activity --username collector 42",
        "user token activity collector 42",
    ] {
        let transport = MockTransport::default();
        respond(&transport, service_account(), None);
        respond(&transport, activity(0, None), None);
        let value = output(
            execute(&transport, &format!("{command} --output json"))
                .await
                .unwrap(),
        );
        assert_eq!(value["session_id"], "family");
        assert_eq!(
            transport.requests()[1].url.path(),
            "/api/v1/iam/principals/7/tokens/42/activity"
        );
    }
}

#[tokio::test]
#[serial]
async fn credential_activity_summary_pipeline_does_not_offer_next_page() {
    let transport = MockTransport::default();
    respond(&transport, service_account(), None);
    respond(&transport, activity(0, Some(1)), None);
    let result = execute(
        &transport,
        "service-account token activity --name collector --token-id 42 | JQ '.sources | length'",
    )
    .await
    .unwrap();
    assert!(result.output.next_page_command.is_none());
    assert!(!result.output.render().contains("More credential sources"));
    assert!(!result.output.warnings.is_empty());
}

#[tokio::test]
#[serial]
async fn object_history_accepts_a_positional_object_after_a_class_selector() {
    for selector in ["--class-id 1", "--class Hosts", "Hosts"] {
        let transport = MockTransport::default();
        if selector != "--class-id 1" {
            respond(&transport, class_history(), None);
        }
        respond(&transport, json!([object_history(2)]), None);
        respond(&transport, json!([object_history(2)]), None);
        let value = output(
            execute(
                &transport,
                &format!("history object {selector} 123 --output json"),
            )
            .await
            .unwrap(),
        );
        assert_eq!(value[0]["id"], 2);
        assert_eq!(
            transport.requests().last().unwrap().url.path(),
            "/api/v1/classes/1/2/history"
        );
    }
}

#[tokio::test]
#[serial]
async fn related_object_lists_preserve_filters_across_pages_and_computed_modes() {
    for options in ["", "--computed all", "--computed all --sort S:rank desc"] {
        let transport = MockTransport::default();
        let fixture: Value =
            from_str(include_str!("../../tests/fixtures/object-show.json")).unwrap();
        let mut class = fixture["root_class"].clone();
        class["collection"] = fixture["collections"][0].clone();
        let mut first = fixture["graph"]["objects"][0].clone();
        first["name"] = json!("host-1");
        first["computed"] = json!({"shared":{"revision":0,"materialization_stale":false,"values":{"rank":2},"errors":{}}});
        let mut second = first.clone();
        second["id"] = json!(999);
        second["name"] = json!("host-2");
        respond(&transport, class, None);
        respond(&transport, json!([first]), Some("next-page"));
        respond(&transport, json!([second]), None);
        let command = format!("object list --class Hosts --related 'room=Server Rooms / ø &=1' --related-id rack=42 --related-depth room=2 --related-where room.name equals 'B 701' --related-where room.data.floor gte 3 --related-where rack.name not_equals retired --where name contains host --limit 1 --all {options} --output json | P name");
        let result = output(execute(&transport, &command).await.unwrap());
        assert_eq!(result, json!([{"name":"host-1"},{"name":"host-2"}]));
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        for request in &requests[1..] {
            let pairs: Vec<_> = request.url.query_pairs().collect();
            for (key, value) in [
                ("related.room.class.name", "Server Rooms / ø &=1"),
                ("related.room.depth__lte", "2"),
                ("related.room.object.name__equals", "B 701"),
                ("related.room.object.json_data__gte", "floor=3"),
                ("related.rack.class.id", "42"),
                ("related.rack.depth__lte", "1"),
                ("related.rack.object.name__not_equals", "retired"),
                ("name__contains", "host"),
            ] {
                assert!(
                    pairs.iter().any(|(k, v)| k == key && v == value),
                    "{key}={value}: {pairs:?}"
                );
            }
            assert_eq!(
                pairs.iter().any(|(k, v)| k == "include" && v == "computed"),
                !options.is_empty()
            );
        }
        assert!(requests[2]
            .url
            .query_pairs()
            .any(|(k, v)| k == "cursor" && v == "next-page"));
    }
}

#[tokio::test]
#[serial]
async fn invalid_related_options_and_resource_ids_fail_before_lookup() {
    for command in [
        "object list --class Hosts --related room=Rooms --related room=Racks",
        "object list --class Hosts --related room=Rooms --related-id room=1",
        "object list --class Hosts --related-id room=0",
        "object list --class Hosts --related bad.alias=Rooms",
        "object list --class Hosts --related room=Rooms --related-depth room=0",
        "object list --class Hosts --related room=Rooms --related-depth room=11",
        "object list --class Hosts --related-depth missing=2",
        "object list --class Hosts --related-where missing.name equals B701",
        "object list --class Hosts --related room=Rooms --related-where room.data..floor equals x",
        "object list --class Hosts --related room=Rooms --related-where room.data.a..b equals x",
        "object list --class Hosts --related room=Rooms --related-where room.name gt x",
        "object list --class Hosts --related room=Rooms --related-where room.S:rank equals 1",
        "object list --class Hosts --related a=A --related b=B --related c=C --related d=D --related e=E",
        "backup show --task 0",
        "import show --id 0",
        "class schema work Hosts --task 0",
        "user token revoke --username alice --token-id 0",
    ] {
        let transport = MockTransport::default();
        assert!(execute(&transport, command).await.is_err(), "{command}");
        assert!(transport.requests().is_empty(), "{command}");
    }
}
