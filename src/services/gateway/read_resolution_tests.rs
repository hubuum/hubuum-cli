use std::sync::Arc;

use hubuum_client::{blocking::Client, Class, MockTransport, Token, TransportResponse};
use reqwest::{header::HeaderValue, StatusCode};
use serde_json::{from_str, from_value, json, to_value, Value};

use crate::list_query::{list_query_from_raw, ListQuery};

use super::{HubuumGateway, RelatedObjectOptions, RelationRoot, RelationTraversalOptions};

fn fixture() -> Value {
    from_str(include_str!("../../../tests/fixtures/object-show.json")).unwrap()
}

fn gateway(responses: impl IntoIterator<Item = Value>) -> (HubuumGateway, MockTransport) {
    let transport = MockTransport::default();
    for response in responses {
        transport.push_response(TransportResponse::json(StatusCode::OK, &response).unwrap());
    }
    let client = Client::builder_from_url("https://example.invalid")
        .unwrap()
        .with_transport(Arc::new(transport.clone()))
        .build()
        .unwrap()
        .authenticate(Token::new("test-token"));
    (HubuumGateway::new(Arc::new(client)), transport)
}

fn traversal() -> RelationTraversalOptions {
    RelationTraversalOptions {
        include_self_class: false,
        max_depth: 2,
    }
}

#[test]
fn object_show_preserves_all_fields_with_three_data_requests() {
    let f = fixture();
    let (gateway, transport) = gateway([
        f["root_class"].clone(),
        f["graph"].clone(),
        f["classes"].clone(),
    ]);
    let details = gateway
        .object_show_details("Hosts", "nommo.uio.no", &traversal(), false)
        .unwrap();
    assert_eq!(to_value(details).unwrap(), f["expected"]);
    let requests = transport.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[1].url.path(),
        "/api/v1/classes/by-name/Hosts/objects/by-name/nommo.uio.no/related/graph"
    );
    assert!(requests[1]
        .url
        .query_pairs()
        .any(|(key, value)| key == "depth__lte" && value == "2"));
    let ids = requests[2]
        .url
        .query_pairs()
        .find(|(key, _)| key == "id__equals")
        .unwrap()
        .1;
    let mut ids = ids.split(',').collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["10", "11", "12"]);
}

#[test]
fn object_show_fetches_only_missing_collections_and_preserves_computed_fields() {
    let mut f = fixture();
    f["graph"]["objects"][0]["collection_id"] = json!(99);
    f["graph"]["objects"][2]["collection_id"] = json!(99);
    let mut collection = f["collections"][0].clone();
    collection["id"] = json!(99);
    collection["name"] = json!("Custom");
    let mut computed_object = f["graph"]["objects"][0].clone();
    computed_object["computed"] = json!({
        "shared": {"revision": 3, "materialization_stale": false, "values": {"memory": 32}, "errors": {}},
        "personal": {"values": {"note": "keep me"}, "errors": {"broken": {"code": "invalid", "path": null, "message": "missing input"}}}
    });
    let (gateway, transport) = gateway([
        f["root_class"].clone(),
        f["graph"].clone(),
        f["classes"].clone(),
        json!([collection]),
        computed_object.clone(),
    ]);
    let details = gateway
        .object_show_details("Hosts", "nommo.uio.no", &traversal(), true)
        .unwrap();
    f["expected"]["collection"] = json!("Custom");
    f["expected"]["related_objects"][1]["collection"] = json!("Custom");
    f["expected"]["computed"] = computed_object["computed"].clone();
    assert_eq!(to_value(details).unwrap(), f["expected"]);
    let requests = transport.requests();
    assert_eq!(requests.len(), 5);
    assert!(requests[3]
        .url
        .query_pairs()
        .any(|(key, value)| key == "id__equals" && value == "99"));
    assert!(requests[4]
        .url
        .query_pairs()
        .any(|(key, value)| key == "include" && value == "computed"));
}

#[test]
fn rootless_graph_falls_back_without_losing_null_data() {
    let mut f = fixture();
    let mut root = f["graph"]["objects"][0].clone();
    root["data"] = Value::Null;
    let (gateway, transport) = gateway([
        f["root_class"].clone(),
        json!({"objects": [], "relations": []}),
        json!([root]),
        json!([f["collections"][0]]),
    ]);
    let details = gateway
        .object_show_details("Hosts", "nommo.uio.no", &traversal(), false)
        .unwrap();
    f["expected"]["data"] = Value::Null;
    f["expected"]["related_objects"] = json!([]);
    assert_eq!(to_value(details).unwrap(), f["expected"]);
    assert_eq!(transport.requests().len(), 4);
}

#[test]
fn object_lists_reuse_the_class_for_plain_computed_and_sorted_output() {
    for (include_computed, sorted) in [(false, false), (true, false), (true, true)] {
        let f = fixture();
        let mut class = f["root_class"].clone();
        class["collection"] = f["collections"][0].clone();
        let mut object = f["graph"]["objects"][0].clone();
        object["computed"] = json!({"shared": {"revision": 1, "materialization_stale": false, "values": {"rank": 2}, "errors": {}}});
        let (gateway, transport) = gateway([class, json!([object.clone()])]);
        let sorts = if sorted {
            vec!["S:rank desc".to_string()]
        } else {
            vec![]
        };
        let query =
            list_query_from_raw(&["class equals Hosts".to_string()], &sorts, None, None).unwrap();
        let page = gateway.list_objects(&query, include_computed).unwrap();
        let mut expected = f["expected"].clone();
        expected.as_object_mut().unwrap().remove("related_objects");
        if include_computed {
            expected["computed"] = object["computed"].clone();
        }
        assert_eq!(to_value(&page.items).unwrap(), json!([expected]));
        assert_eq!(transport.requests().len(), 2);
        assert_eq!(page.returned_count, 1);
    }
}

#[test]
fn class_show_reuses_its_embedded_collection() {
    let f = fixture();
    let mut class = f["root_class"].clone();
    class["collection"] = f["collections"][0].clone();
    let (gateway, transport) = gateway([
        class.clone(),
        json!([]),
        json!({"classes": [], "relations": []}),
    ]);
    let details = gateway.class_show_details("Hosts", &traversal()).unwrap();
    assert_eq!(details.class.0, from_value::<Class>(class).unwrap());
    assert_eq!(transport.requests().len(), 3);
}

#[test]
fn missing_collection_resolution_keeps_pagination() {
    let f = fixture();
    let (gateway, transport) = gateway([]);
    let mut first = TransportResponse::json(StatusCode::OK, &json!([f["collections"][0]])).unwrap();
    first
        .headers
        .insert("x-next-cursor", HeaderValue::from_static("page-2"));
    transport.push_response(first);
    transport.push_response(
        TransportResponse::json(StatusCode::OK, &json!([f["collections"][1]])).unwrap(),
    );
    let classes = [from_value::<Class>(f["root_class"].clone()).unwrap()];
    let collections = gateway
        .collection_map_with_classes([7, 8, 7], &classes)
        .unwrap();
    assert_eq!(collections[&7].name, "Math");
    assert_eq!(collections[&8].name, "Facilities");
    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1]
        .url
        .query_pairs()
        .any(|(key, value)| key == "cursor" && value == "page-2"));
}

#[test]
fn related_object_output_reuses_embedded_collections_and_keeps_paths() {
    let f = fixture();
    let objects = &f["graph"]["objects"];
    let (gateway, transport) = gateway([
        f["root_class"].clone(),
        json!([objects[0]]),
        json!([objects[1], objects[2], objects[3]]),
        f["classes"].clone(),
    ]);
    let page = gateway
        .list_related_objects(
            &RelationRoot {
                root_class: "Hosts".to_string(),
                root_object: "nommo.uio.no".to_string(),
            },
            &RelatedObjectOptions::default(),
            &ListQuery::default(),
        )
        .unwrap();
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.items[2].class, "Rooms");
    assert_eq!(page.items[2].collection, "Facilities");
    assert_eq!(page.items[2].path, ["BL14=521.A7-UD7056", "B701"]);
    assert_eq!(transport.requests().len(), 4);
}

#[test]
fn delegated_sink_lookup_uses_only_the_collection_discovery_endpoint() {
    let sink = json!({"id": 5, "name": "notifications", "kind": "webhook", "enabled": true, "collection_id": 7, "revision": 1, "routing": "fixed"});
    let (gateway, transport) = gateway([json!([sink])]);
    assert_eq!(
        gateway
            .collection_event_sink_id_by_name(7.into(), "notifications")
            .unwrap(),
        5
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/api/v1/collections/7/event-sinks");
}

#[test]
fn subscription_sink_lookup_does_not_fall_back_on_permission_denial() {
    let (gateway, transport) = gateway([]);
    transport.push_response(TransportResponse::empty(StatusCode::FORBIDDEN));
    assert!(gateway
        .subscription_sink_id_by_name(7.into(), "notifications")
        .is_err());
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn subscription_sink_lookup_preserves_legacy_admin_workflows() {
    let (gateway, transport) = gateway([]);
    transport.push_response(TransportResponse::empty(StatusCode::NOT_FOUND));
    transport.push_response(
        TransportResponse::json(
            StatusCode::OK,
            &json!([{
                "id":5,"name":"notifications","kind":"webhook","config":{},"enabled":false,
                "created_at":"2026-10-05T00:00:00Z","updated_at":"2026-10-05T00:00:00Z","revision":1
            }]),
        )
        .unwrap(),
    );
    assert_eq!(
        gateway
            .subscription_sink_id_by_name(7.into(), "notifications")
            .unwrap(),
        5
    );
    assert_eq!(transport.requests()[1].url.path(), "/api/v1/event-sinks");
}

#[test]
fn scoped_sink_discovery_rejects_unsupported_queries_before_sending() {
    use crate::list_query::{FilterClause, SortClause, SortDirectionArg};
    use hubuum_client::FilterOperator;
    for field in ["enabled", "updated_at"] {
        for sort in [false, true] {
            let (gateway, transport) = gateway([]);
            let query = if sort {
                ListQuery {
                    sorts: vec![SortClause {
                        field: field.into(),
                        direction: SortDirectionArg::Asc,
                    }],
                    ..Default::default()
                }
            } else {
                ListQuery {
                    filters: vec![FilterClause {
                        field: field.into(),
                        operator: FilterOperator::Equals { is_negated: false },
                        value: "true".into(),
                    }],
                    ..Default::default()
                }
            };
            assert!(gateway.collection_event_sinks(7.into(), &query).is_err());
            assert!(transport.requests().is_empty());
        }
    }
}
