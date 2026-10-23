use std::sync::Arc;

use hubuum_client::{
    blocking::Client, EventNotificationRequest, MockTransport, Token, TransportResponse,
};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use super::{AuditListInput, AuditScope, EventSubscriptionScope, HubuumGateway};
use crate::list_query::ListQuery;

fn gateway(transport: &MockTransport) -> HubuumGateway {
    HubuumGateway::new(Arc::new(
        Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("secret")),
    ))
}

fn respond(transport: &MockTransport, value: Value) {
    transport.push_response(TransportResponse::json(StatusCode::OK, &value).unwrap());
}

#[test]
fn system_subscription_names_are_exact_and_never_fall_back_to_collection_routes() {
    let transport = MockTransport::default();
    respond(
        &transport,
        json!([{
            "id":7,"sink_id":9,"name":"login alerts","description":"",
            "entity_types":["authentication"],"actions":["succeeded"],"routing":{},
            "enabled":false,"created_at":"2026-10-13T00:00:00Z","updated_at":"2026-10-13T00:00:00Z","revision":1
        }]),
    );
    assert!(gateway(&transport)
        .event_subscription_by_name(EventSubscriptionScope::System, "login")
        .is_err());
    assert_eq!(transport.requests().len(), 1);
    assert_eq!(
        transport.requests()[0].url.path(),
        "/api/v1/system-event-subscriptions"
    );

    let transport = MockTransport::default();
    transport.push_response(
        TransportResponse::json(StatusCode::FORBIDDEN, &json!({"message":"forbidden"})).unwrap(),
    );
    assert!(gateway(&transport)
        .event_subscriptions(EventSubscriptionScope::System, &ListQuery::default())
        .is_err());
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn system_list_rejects_unsupported_pagination_before_network_access() {
    let transport = MockTransport::default();
    for query in [
        ListQuery {
            limit: Some(1.try_into().expect("positive page size")),
            ..Default::default()
        },
        ListQuery {
            cursor: Some("next".into()),
            ..Default::default()
        },
    ] {
        assert!(gateway(&transport)
            .event_subscriptions(EventSubscriptionScope::System, &query)
            .unwrap_err()
            .to_string()
            .contains("complete list"));
    }
    assert!(transport.requests().is_empty());
    respond(&transport, json!([]));
    let page = gateway(&transport)
        .event_subscriptions(
            EventSubscriptionScope::System,
            &ListQuery {
                include_total: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page.total_count, Some(0));
    assert!(page.next_cursor.is_none());
}

#[test]
fn related_audit_is_opt_in_and_limited_to_class_or_object_scope() {
    let transport = MockTransport::default();
    let gateway = gateway(&transport);
    let input = AuditListInput {
        include_relations: true,
        ..Default::default()
    };
    assert!(gateway
        .audit_events(AuditScope::Global, input.clone())
        .is_err());
    assert!(gateway
        .audit_events(AuditScope::Collection(3.into()), input.clone())
        .is_err());
    assert!(transport.requests().is_empty());
    respond(&transport, json!([]));
    respond(&transport, json!([]));
    gateway
        .audit_events(AuditScope::Class(42.into()), input)
        .unwrap();
    gateway
        .audit_events(
            AuditScope::Object {
                class_id: 42.into(),
                object_id: 7.into(),
            },
            AuditListInput::default(),
        )
        .unwrap();
    let requests = transport.requests();
    assert_eq!(requests[0].url.path(), "/api/v1/classes/42/events");
    assert_eq!(requests[0].url.query(), Some("include_relations=true"));
    assert!(!requests[1]
        .url
        .query_pairs()
        .any(|(key, _)| key == "include_relations"));
}

#[test]
fn preview_and_test_use_distinct_typed_routes_and_preserve_source_identity() {
    let transport = MockTransport::default();
    respond(
        &transport,
        json!({"sink_kind":"webhook","payload":{"text":"preview"}}),
    );
    respond(
        &transport,
        json!({"id":1,"event_id":2,"subscription_id":7,"status":"pending","purpose":"test",
        "attempts":0,"next_attempt_at":"2026-10-13T00:00:00Z","created_at":"2026-10-13T00:00:00Z","updated_at":"2026-10-13T00:00:00Z"}),
    );
    let request = EventNotificationRequest {
        subscription_id: 7.into(),
        event_id: "11111111-1111-4111-8111-111111111111".into(),
    };
    let gateway = gateway(&transport);
    assert_eq!(
        gateway
            .preview_event_notification(9.into(), request.clone())
            .unwrap()
            .value["payload"]["text"],
        "preview"
    );
    assert_eq!(
        gateway
            .test_event_notification(9.into(), request)
            .unwrap()
            .value["purpose"],
        "test"
    );
    let requests = transport.requests();
    assert_eq!(requests[0].url.path(), "/api/v1/event-sinks/9/preview");
    assert_eq!(requests[1].url.path(), "/api/v1/event-sinks/9/test");
    for request in requests {
        assert_eq!(request.method, Method::POST);
        let value: Value = serde_json::from_slice(request.body()).unwrap();
        assert_eq!(value["subscription_id"], 7);
        assert_eq!(value["event_id"], "11111111-1111-4111-8111-111111111111");
    }
}
