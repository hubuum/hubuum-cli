use std::collections::{HashMap, HashSet};

use hubuum_client::{
    Class, ClassId, Collection, CollectionId, Object, ObjectId, PrincipalTokenMetadata, TokenId,
    TokenResourceScope,
};

use crate::domain::{PrincipalTokenDetailsRecord, ResolvedTokenResource, TokenResourceParent};
use crate::errors::AppError;

use super::{
    shared::{class_collection_id, fetch_entities_for_ids},
    HubuumGateway,
};

const MAX_FILTERED_OBJECT_IDS_PER_CLASS: usize = 50;

#[derive(Debug, Default)]
struct TokenScopeIds {
    collections: Vec<CollectionId>,
    classes: Vec<ClassId>,
    objects: Vec<ObjectId>,
}

impl TokenScopeIds {
    fn from_resources(resources: &[TokenResourceScope]) -> Self {
        let mut ids = Self::default();
        for resource in resources {
            match resource {
                TokenResourceScope::Collection(id) => ids.collections.push(*id),
                TokenResourceScope::Class(id) => ids.classes.push(*id),
                TokenResourceScope::Object(id) => ids.objects.push(*id),
            }
        }
        ids
    }

    fn include_parent_collections<'a>(
        &mut self,
        classes: impl IntoIterator<Item = &'a Class>,
        objects: impl IntoIterator<Item = &'a Object>,
    ) {
        self.collections
            .extend(classes.into_iter().filter_map(class_collection_id));
        self.collections
            .extend(objects.into_iter().map(|object| object.collection_id));
    }
}

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

        let mut ids = TokenScopeIds::from_resources(&resources);
        let mut resolver = TokenScopeResolver::new(self);
        resolver.resolve_classes(&ids.classes)?;
        resolver.resolve_objects_from_scoped_classes(&ids.classes, &ids.objects)?;
        ids.include_parent_collections(resolver.classes.values(), resolver.objects.values());
        resolver.resolve_collections(&ids.collections)?;

        let resolved_resources = resolve_token_resources(
            &resources,
            &resolver.collections,
            &resolver.classes,
            &resolver.objects,
        );
        Ok(PrincipalTokenDetailsRecord::new(token, resolved_resources))
    }
}

struct TokenScopeResolver<'a> {
    gateway: &'a HubuumGateway,
    collections: HashMap<CollectionId, Collection>,
    classes: HashMap<ClassId, Class>,
    objects: HashMap<ObjectId, Object>,
    probed_collection_ids: HashSet<CollectionId>,
    probed_class_ids: HashSet<ClassId>,
    probed_object_ids_by_class: HashMap<ClassId, HashSet<ObjectId>>,
    fully_scanned_object_classes: HashSet<ClassId>,
}

impl<'a> TokenScopeResolver<'a> {
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
            .copied()
            .filter(|id| !self.probed_collection_ids.contains(id))
            .collect::<HashSet<_>>();
        let found = fetch_entities_for_ids(
            &self.gateway.client.collections(),
            unprobed_ids.iter().copied(),
        )?;

        self.probed_collection_ids.extend(unprobed_ids);
        self.collections.extend(
            found
                .into_values()
                .map(|collection| (collection.id, collection)),
        );
        Ok(())
    }

    fn resolve_classes(&mut self, class_ids: &[ClassId]) -> Result<(), AppError> {
        let unprobed_ids = class_ids
            .iter()
            .copied()
            .filter(|id| !self.probed_class_ids.contains(id))
            .collect::<HashSet<_>>();
        let found =
            fetch_entities_for_ids(&self.gateway.client.classes(), unprobed_ids.iter().copied())?;

        self.probed_class_ids.extend(unprobed_ids);
        self.classes
            .extend(found.into_values().map(|class| (class.id, class)));
        Ok(())
    }

    fn resolve_objects_from_scoped_classes(
        &mut self,
        class_ids: &[ClassId],
        object_ids: &[ObjectId],
    ) -> Result<(), AppError> {
        let requested_ids = object_ids.iter().copied().collect::<HashSet<_>>();

        for class_id in class_ids {
            if requested_ids
                .iter()
                .all(|object_id| self.objects.contains_key(object_id))
            {
                break;
            }
            if self.fully_scanned_object_classes.contains(class_id) {
                continue;
            }
            let previously_probed = self
                .probed_object_ids_by_class
                .get(class_id)
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
                    .entry(*class_id)
                    .or_default()
                    .extend(unprobed_ids);
                self.objects
                    .extend(found.into_values().map(|object| (object.id, object)));
            } else {
                let found = self.gateway.client.objects(*class_id).query().all()?;
                self.fully_scanned_object_classes.insert(*class_id);
                self.probed_object_ids_by_class.remove(class_id);
                self.objects
                    .extend(found.into_iter().map(|object| (object.id, object)));
            }
        }

        Ok(())
    }
}

fn resolve_token_resources(
    resources: &[TokenResourceScope],
    collections: &HashMap<CollectionId, Collection>,
    classes: &HashMap<ClassId, Class>,
    objects: &HashMap<ObjectId, Object>,
) -> Vec<ResolvedTokenResource> {
    resources
        .iter()
        .map(|resource| match resource {
            TokenResourceScope::Collection(id) => collections.get(id).map_or_else(
                || ResolvedTokenResource::unresolved_collection(*id),
                |collection| {
                    ResolvedTokenResource::resolved_collection(*id, collection.name.clone())
                },
            ),
            TokenResourceScope::Class(id) => classes.get(id).map_or_else(
                || ResolvedTokenResource::unresolved_class(*id),
                |class| match class_collection_id(class) {
                    Some(collection_id) => {
                        let collection = TokenResourceParent::new(
                            collection_id,
                            collections
                                .get(&collection_id)
                                .map(|collection| collection.name.clone()),
                        );
                        ResolvedTokenResource::resolved_class(*id, class.name.clone(), collection)
                    }
                    None => ResolvedTokenResource::resolved_class_without_collection(
                        *id,
                        class.name.clone(),
                    ),
                },
            ),
            TokenResourceScope::Object(id) => objects.get(id).map_or_else(
                || ResolvedTokenResource::unreachable_object(*id),
                |object| {
                    let class = TokenResourceParent::new(
                        object.hubuum_class_id,
                        classes
                            .get(&object.hubuum_class_id)
                            .map(|class| class.name.clone()),
                    );
                    let collection = TokenResourceParent::new(
                        object.collection_id,
                        collections
                            .get(&object.collection_id)
                            .map(|collection| collection.name.clone()),
                    );
                    ResolvedTokenResource::resolved_object(
                        *id,
                        object.name.clone(),
                        class,
                        collection,
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

    use super::{resolve_token_resources, HubuumGateway, TokenScopeResolver};

    fn collection(id: i32, name: &str) -> Collection {
        from_value(json!({
            "id": id,
            "name": name,
            "description": "",
            "parent_collection_id": null,
            "revision": 1,
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
            "revision": 1,
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
            "revision": 1,
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
            &HashMap::from([(7.into(), collection)]),
            &HashMap::from([(8.into(), class)]),
            &HashMap::from([(9.into(), object)]),
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
        let mut resolver = TokenScopeResolver::new(&gateway);
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
        let mut resolver = TokenScopeResolver::new(&gateway);
        let class_ids = vec![1.into()];
        let object_ids = vec![9.into(), 10.into()];

        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("first resolution should succeed");
        resolver
            .resolve_objects_from_scoped_classes(&class_ids, &object_ids)
            .expect("cached resolution should succeed");

        assert_eq!(
            resolver
                .objects
                .get(&9.into())
                .map(|object| object.name.as_str()),
            Some("host.example.org")
        );
        assert_eq!(transport.requests().len(), 1);
    }
}
