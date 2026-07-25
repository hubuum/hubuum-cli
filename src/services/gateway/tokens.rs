use std::collections::{HashMap, HashSet};

use hubuum_client::{
    Class, ClassId, Collection, CollectionId, Object, ObjectId, PrincipalTokenMetadata, TokenId,
    TokenResourceScope,
};

use crate::domain::{PrincipalTokenDetailsRecord, ResolvedTokenResource};
use crate::errors::AppError;

use super::{shared::fetch_entities_for_ids, HubuumGateway};

const MAX_FILTERED_OBJECT_IDS_PER_CLASS: usize = 50;

impl HubuumGateway {
    pub(super) fn principal_token_details(
        &self,
        tokens: impl IntoIterator<Item = PrincipalTokenMetadata>,
        token_id: TokenId,
    ) -> Result<PrincipalTokenDetailsRecord, AppError> {
        let token = tokens
            .into_iter()
            .find(|token| token.id == token_id)
            .ok_or_else(|| AppError::EntityNotFound(format!("token {token_id}")))?;
        let resources = token
            .scope
            .as_ref()
            .and_then(|scope| scope.resources())
            .unwrap_or_default()
            .to_vec();

        let mut collection_ids = resources
            .iter()
            .filter_map(|resource| match resource {
                TokenResourceScope::Collection(id) => Some(*id),
                TokenResourceScope::Class(_) | TokenResourceScope::Object(_) => None,
            })
            .collect::<Vec<_>>();
        let class_ids = resources
            .iter()
            .filter_map(|resource| match resource {
                TokenResourceScope::Class(id) => Some(*id),
                TokenResourceScope::Collection(_) | TokenResourceScope::Object(_) => None,
            })
            .collect::<Vec<_>>();
        let object_ids = resources
            .iter()
            .filter_map(|resource| match resource {
                TokenResourceScope::Object(id) => Some(*id),
                TokenResourceScope::Collection(_) | TokenResourceScope::Class(_) => None,
            })
            .collect::<Vec<_>>();

        let mut resolver = CommandIdResolutionCache::new(self);
        resolver.resolve_classes(&class_ids)?;
        resolver.resolve_objects_from_scoped_classes(&class_ids, &object_ids)?;
        collection_ids.extend(resolver.classes.values().map(|class| class.collection.id));
        collection_ids.extend(resolver.objects.values().map(|object| object.collection_id));
        resolver.resolve_collections(&collection_ids)?;

        let resolved_resources = resolve_token_resources(
            &resources,
            &resolver.collections,
            &resolver.classes,
            &resolver.objects,
        );
        Ok(PrincipalTokenDetailsRecord::new(token, resolved_resources))
    }
}

struct CommandIdResolutionCache<'a> {
    gateway: &'a HubuumGateway,
    collections: HashMap<i32, Collection>,
    classes: HashMap<i32, Class>,
    objects: HashMap<i32, Object>,
    probed_collection_ids: HashSet<i32>,
    probed_class_ids: HashSet<i32>,
    probed_object_ids_by_class: HashMap<i32, HashSet<i32>>,
    fully_scanned_object_classes: HashSet<i32>,
}

impl<'a> CommandIdResolutionCache<'a> {
    fn new(gateway: &'a HubuumGateway) -> Self {
        Self {
            gateway,
            collections: HashMap::new(),
            classes: HashMap::new(),
            objects: HashMap::new(),
            probed_collection_ids: HashSet::new(),
            probed_class_ids: HashSet::new(),
            probed_object_ids_by_class: HashMap::new(),
            fully_scanned_object_classes: HashSet::new(),
        }
    }

    fn resolve_collections(&mut self, collection_ids: &[CollectionId]) -> Result<(), AppError> {
        let unprobed_ids = collection_ids
            .iter()
            .map(|id| id.get())
            .filter(|id| !self.probed_collection_ids.contains(id))
            .collect::<HashSet<_>>();
        let found = fetch_entities_for_ids(
            &self.gateway.client.collections(),
            unprobed_ids.iter().copied(),
        )?;

        self.probed_collection_ids.extend(unprobed_ids);
        self.collections.extend(found);
        Ok(())
    }

    fn resolve_classes(&mut self, class_ids: &[ClassId]) -> Result<(), AppError> {
        let unprobed_ids = class_ids
            .iter()
            .map(|id| id.get())
            .filter(|id| !self.probed_class_ids.contains(id))
            .collect::<HashSet<_>>();
        let found =
            fetch_entities_for_ids(&self.gateway.client.classes(), unprobed_ids.iter().copied())?;

        self.probed_class_ids.extend(unprobed_ids);
        self.classes.extend(found);
        Ok(())
    }

    fn resolve_objects_from_scoped_classes(
        &mut self,
        class_ids: &[ClassId],
        object_ids: &[ObjectId],
    ) -> Result<(), AppError> {
        let requested_ids = object_ids.iter().map(|id| id.get()).collect::<HashSet<_>>();

        for class_id in class_ids {
            if requested_ids
                .iter()
                .all(|object_id| self.objects.contains_key(object_id))
            {
                break;
            }
            let class_id_value = class_id.get();
            if self.fully_scanned_object_classes.contains(&class_id_value) {
                continue;
            }
            let previously_probed = self
                .probed_object_ids_by_class
                .get(&class_id_value)
                .cloned()
                .unwrap_or_default();
            let unprobed_ids = requested_ids
                .iter()
                .filter(|object_id| {
                    !self.objects.contains_key(object_id) && !previously_probed.contains(object_id)
                })
                .copied()
                .collect::<HashSet<_>>();
            if unprobed_ids.is_empty() {
                continue;
            }

            if unprobed_ids.len() <= MAX_FILTERED_OBJECT_IDS_PER_CLASS {
                let found = fetch_entities_for_ids(
                    &self.gateway.client.objects(*class_id),
                    unprobed_ids.iter().copied(),
                )?;
                self.probed_object_ids_by_class
                    .entry(class_id_value)
                    .or_default()
                    .extend(unprobed_ids);
                self.objects.extend(found);
            } else {
                let found = self.gateway.client.objects(*class_id).query().all()?;
                self.fully_scanned_object_classes.insert(class_id_value);
                self.probed_object_ids_by_class.remove(&class_id_value);
                self.objects
                    .extend(found.into_iter().map(|object| (object.id.get(), object)));
            }
        }

        Ok(())
    }
}

fn resolve_token_resources(
    resources: &[TokenResourceScope],
    collections: &HashMap<i32, Collection>,
    classes: &HashMap<i32, Class>,
    objects: &HashMap<i32, Object>,
) -> Vec<ResolvedTokenResource> {
    resources
        .iter()
        .map(|resource| match resource {
            TokenResourceScope::Collection(id) => collections.get(&id.get()).map_or_else(
                || ResolvedTokenResource::unresolved_collection(*id),
                |collection| {
                    ResolvedTokenResource::resolved_collection(*id, collection.name.clone())
                },
            ),
            TokenResourceScope::Class(id) => classes.get(&id.get()).map_or_else(
                || ResolvedTokenResource::unresolved_class(*id),
                |class| {
                    ResolvedTokenResource::resolved_class(
                        *id,
                        class.name.clone(),
                        class.collection.id,
                        collections
                            .get(&class.collection.id.get())
                            .map(|collection| collection.name.clone()),
                    )
                },
            ),
            TokenResourceScope::Object(id) => objects.get(&id.get()).map_or_else(
                || ResolvedTokenResource::unreachable_object(*id),
                |object| {
                    ResolvedTokenResource::resolved_object(
                        *id,
                        object.name.clone(),
                        object.hubuum_class_id,
                        classes
                            .get(&object.hubuum_class_id.get())
                            .map(|class| class.name.clone()),
                        object.collection_id,
                        collections
                            .get(&object.collection_id.get())
                            .map(|collection| collection.name.clone()),
                    )
                },
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use hubuum_client::{
        blocking::Client, Class, Collection, MockTransport, Object, Token, TokenResourceScope,
        TransportResponse,
    };
    use reqwest::StatusCode;
    use serde_json::{from_value, json, to_value};

    use super::{resolve_token_resources, CommandIdResolutionCache, HubuumGateway};

    fn collection(id: i32, name: &str) -> Collection {
        from_value(json!({
            "id": id,
            "name": name,
            "description": "",
            "parent_collection_id": null,
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        }))
        .expect("collection fixture should deserialize")
    }

    fn class(id: i32, collection: &Collection, name: &str) -> Class {
        from_value(json!({
            "id": id,
            "name": name,
            "description": "",
            "collection": collection,
            "json_schema": null,
            "validate_schema": null,
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        }))
        .expect("class fixture should deserialize")
    }

    fn object(id: i32, class_id: i32, collection_id: i32, name: &str) -> Object {
        from_value(json!({
            "id": id,
            "name": name,
            "collection_id": collection_id,
            "hubuum_class_id": class_id,
            "description": "",
            "data": {},
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        }))
        .expect("object fixture should deserialize")
    }

    #[test]
    fn object_scopes_not_found_in_scoped_classes_are_unreachable() {
        let collection = collection(7, "Infrastructure");
        let class = class(8, &collection, "Hosts");
        let object = object(9, 8, 7, "host.example.org");
        let resources = vec![
            TokenResourceScope::Collection(7.into()),
            TokenResourceScope::Class(8.into()),
            TokenResourceScope::Object(9.into()),
            TokenResourceScope::Object(10.into()),
        ];
        let resolved = resolve_token_resources(
            &resources,
            &HashMap::from([(7, collection)]),
            &HashMap::from([(8, class)]),
            &HashMap::from([(9, object)]),
        );
        let value = to_value(resolved).expect("resolved scopes should serialize");

        assert_eq!(value[2]["resolution"], "resolved");
        assert_eq!(value[2]["name"], "host.example.org");
        assert_eq!(value[2]["class_name"], "Hosts");
        assert_eq!(value[3]["resolution"], "unreachable");
        assert_eq!(value[3]["id"], 10);
    }

    #[test]
    fn large_object_scopes_scan_each_class_once_and_cache_the_result() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &json!([]))
                .expect("first class response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &json!([]))
                .expect("second class response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should be valid")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let mut resolver = CommandIdResolutionCache::new(&gateway);
        let class_ids = vec![1.into(), 2.into()];
        let object_ids = (1..=60).map(Into::into).collect::<Vec<_>>();

        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("first resolution should succeed");
        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("cached resolution should succeed");

        let requests = transport.requests();
        assert_eq!(requests.len(), class_ids.len());
        assert!(requests
            .iter()
            .all(|request| !request.url.as_str().contains("id=")));
    }

    #[test]
    fn filtered_object_scope_hits_and_misses_are_cached_for_the_command() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &[object(9, 1, 1, "host.example.org")])
                .expect("class response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should be valid")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let mut resolver = CommandIdResolutionCache::new(&gateway);
        let class_ids = vec![1.into()];
        let object_ids = vec![9.into(), 10.into()];

        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("first resolution should succeed");
        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("cached resolution should succeed");

        assert_eq!(
            resolver.objects.get(&9).map(|object| object.name.as_str()),
            Some("host.example.org")
        );
        assert_eq!(transport.requests().len(), 1);
    }
}
