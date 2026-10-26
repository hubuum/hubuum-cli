use super::*;

fn class(transport: &MockTransport) {
    let fixture: Value =
        from_str(include_str!("../../../tests/fixtures/object-show.json")).unwrap();
    respond(transport, fixture["root_class"].clone(), None);
}

fn task(status: &str) -> Value {
    json!({"id":71, "kind":"schema_profiling", "status":status, "created_at":AT,
        "progress":{"total_items":1,"processed_items":1,"success_items":1,"failed_items":0},
        "links":{"task":"/api/v1/tasks/71","events":"/api/v1/tasks/71/events"},
        "details":{"schema_profiling":{"class_id":9,"output_url":"/api/v1/classes/9/schema/profiles/71"}}})
}

fn report(preset: &str, status: &str) -> Value {
    json!({"class_id":9,"sampled":true,"sampled_objects":1,"effective_sample_limit":7,
        "options":{"preset":preset,"sample_limit":7},"status":status,"sample_valid":status == "candidate",
        "schema":{"type":"object","properties":{"hostname":{"type":"string"}}},"findings":[]})
}

#[tokio::test]
#[serial]
async fn suggestions_wait_for_success_and_preserve_the_task_id_without_staging() {
    for preset in ["strict", "balanced", "expansive"] {
        let transport = MockTransport::default();
        class(&transport);
        respond(&transport, task("queued"), None);
        respond(&transport, task("succeeded"), None);
        respond(&transport, report(preset, "candidate"), None);
        let rows = output(execute(&transport, &format!("class schema suggest Hosts --preset {preset} --limit 7 --wait --idempotency-key test-profile --output json")).await.unwrap());
        assert_eq!(rows["task_id"], 71);
        assert_eq!(rows["options"]["preset"], preset);
        assert_eq!(rows["sample_valid"], true);
        let requests = transport.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1].url.path(), "/api/v1/classes/9/schema/profiles");
        assert_eq!(requests[1].headers["Idempotency-Key"], "test-profile");
        assert_eq!(
            serde_json::from_slice::<Value>(requests[1].body()).unwrap(),
            json!({"preset":preset,"sample_limit":7})
        );
        assert_eq!(
            requests[3].url.path(),
            "/api/v1/classes/9/schema/profiles/71"
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == Method::POST)
                .count(),
            1
        );
    }
}

#[tokio::test]
#[serial]
async fn suggestion_submission_returns_immediately_without_wait() {
    let transport = MockTransport::default();
    class(&transport);
    respond(&transport, task("queued"), None);
    let result = output(
        execute(&transport, "class schema suggest Hosts --output json")
            .await
            .unwrap(),
    );
    assert_eq!(result["id"], 71);
    assert_eq!(result["kind"], "schema_profiling");
    assert_eq!(transport.requests().len(), 2);
}

#[tokio::test]
#[serial]
async fn failed_or_partial_suggestions_never_download_reports() {
    for status in ["failed", "cancelled", "partially_succeeded"] {
        let transport = MockTransport::default();
        class(&transport);
        respond(&transport, task("queued"), None);
        respond(&transport, task(status), None);
        let error = execute(&transport, "class schema suggest Hosts --wait")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("71"));
        assert_eq!(transport.requests().len(), 3);
    }
}

#[tokio::test]
#[serial]
async fn staging_a_reviewed_suggestion_requires_explicit_policy_and_never_activates() {
    let transport = MockTransport::default();
    class(&transport);
    let candidate = report("balanced", "candidate");
    respond(&transport, candidate.clone(), None);
    respond(
        &transport,
        json!({"class_id":9,"revision":2,"status":"staged","validate_schema":true,"json_schema":candidate["schema"],"created_at":AT}),
        None,
    );
    let result = output(
        execute(
            &transport,
            "class schema stage Hosts --from-suggestion 71 --validate true --output json",
        )
        .await
        .unwrap(),
    );
    assert_eq!(result["revision"], 2);
    let requests = transport.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[1].url.path(),
        "/api/v1/classes/9/schema/profiles/71"
    );
    assert_eq!(requests[2].url.path(), "/api/v1/classes/9/schema/revisions");
    assert_eq!(
        serde_json::from_slice::<Value>(requests[2].body()).unwrap(),
        json!({"json_schema":candidate["schema"],"validate_schema":true})
    );
}

#[tokio::test]
#[serial]
async fn unusable_or_wrong_class_reports_cannot_be_staged() {
    for (status, class_id) in [("blocked", 9), ("insufficient", 9), ("candidate", 42)] {
        let transport = MockTransport::default();
        class(&transport);
        let mut candidate = report("strict", status);
        candidate["class_id"] = json!(class_id);
        respond(&transport, candidate, None);
        assert!(execute(
            &transport,
            "class schema stage Hosts --from-suggestion 71 --validate true"
        )
        .await
        .is_err());
        assert_eq!(transport.requests().len(), 2);
    }
}

#[tokio::test]
#[serial]
async fn suggestion_options_are_validated_before_requests() {
    let transport = MockTransport::default();
    for command in [
        "class schema suggest Hosts --limit 0",
        "class schema suggest Hosts --limit 251",
        "class schema suggest Hosts --preset unknown",
        "class schema suggest Hosts --timeout 5",
        "class schema suggest Hosts --wait --timeout 0",
        "class schema suggestion Hosts --task 0",
        "class schema stage Hosts --from-suggestion 71",
        "class schema stage Hosts --from-suggestion 71 --from-revision 2 --validate true",
        "class schema stage Hosts --from-suggestion 71 --schema null --validate false",
    ] {
        assert!(execute(&transport, command).await.is_err(), "{command}");
        assert!(transport.requests().is_empty(), "{command}");
    }
}
