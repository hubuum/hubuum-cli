use std::collections::{HashMap, HashSet};

use hubuum_client::{
    Class, Collection, Object, PrincipalTokenMetadata, TokenId, TokenResourceScope,
};

use crate::domain::{PrincipalTokenDetailsRecord, ResolvedTokenResource};
use crate::errors::AppError;

use super::{shared::fetch_entities_for_ids, HubuumGateway};

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

        let collection_ids = resources.iter().filter_map(|resource| match resource {
            TokenResourceScope::Collection(id) => Some(*id),
            TokenResourceScope::Class(_) | TokenResourceScope::Object(_) => None,
        });
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

        let class_map = fetch_entities_for_ids(&self.client.classes(), class_ids.iter().copied())?;
        let object_map = self.objects_from_scoped_classes(&class_ids, &object_ids)?;
        let collection_map = fetch_entities_for_ids(
            &self.client.collections(),
            collection_ids
                .chain(class_map.values().map(|class| class.collection.id))
                .chain(object_map.values().map(|object| object.collection_id)),
        )?;

        let resolved_resources =
            resolve_token_resources(&resources, &collection_map, &class_map, &object_map);
        Ok(PrincipalTokenDetailsRecord::new(token, resolved_resources))
    }

    fn objects_from_scoped_classes(
        &self,
        class_ids: &[hubuum_client::ClassId],
        object_ids: &[hubuum_client::ObjectId],
    ) -> Result<HashMap<i32, Object>, AppError> {
        let mut remaining_ids = object_ids.iter().map(|id| id.get()).collect::<HashSet<_>>();
        let mut objects = HashMap::new();

        for class_id in class_ids {
            if remaining_ids.is_empty() {
                break;
            }
            let found = fetch_entities_for_ids(
                &self.client.objects(*class_id),
                remaining_ids.iter().copied(),
            )?;
            for (object_id, object) in found {
                remaining_ids.remove(&object_id);
                objects.insert(object_id, object);
            }
        }

        Ok(objects)
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
    use std::collections::HashMap;

    use hubuum_client::{Class, Collection, Object, TokenResourceScope};
    use serde_json::{from_value, json, to_value};

    use super::resolve_token_resources;

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
}
