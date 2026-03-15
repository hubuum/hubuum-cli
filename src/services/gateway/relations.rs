use std::collections::HashMap;

use crate::domain::{
    ResolvedClassRelationRecord, ResolvedObjectRelationRecord, ResolvedRelatedClassGraph,
    ResolvedRelatedClassRecord, ResolvedRelatedObjectGraph, ResolvedRelatedObjectRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    apply_cursor_request_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone)]
pub struct RelationTarget {
    pub class_a: String,
    pub class_b: String,
    pub object_a: Option<String>,
    pub object_b: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RelationRoot {
    pub root_class: String,
    pub root_object: String,
}

#[derive(Debug, Clone, Default)]
pub struct RelatedObjectOptions {
    pub ignore_classes: Vec<String>,
    pub include_self_class: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RelationTraversalOptions {
    pub include_self_class: bool,
    pub max_depth: i32,
}

impl HubuumGateway {
    pub fn list_related_classes(
        &self,
        root_class: &str,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedRelatedClassRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, RELATED_CLASS_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, RELATED_CLASS_SORT_SPECS)?;
        let class = self.class_handle_by_name(root_class)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = apply_cursor_request_paging(
            class.related_classes().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;

        self.resolve_related_class_page(page, query.limit, class.resource())
    }

    pub fn list_related_class_relations(
        &self,
        root_class: &str,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedClassRelationRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, CLASS_RELATION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, CLASS_RELATION_SORT_SPECS)?;
        let class = self.class_handle_by_name(root_class)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = apply_cursor_request_paging(
            class.related_relations().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit: query.limit,
                returned_count: 0,
            });
        }

        let class_map = self.class_map_from_relation_ids(&page.items)?;
        Ok(PagedResult::from_page(page, query.limit, |relation| {
            ResolvedClassRelationRecord::new(&relation, &class_map)
        }))
    }

    pub fn related_class_graph(
        &self,
        root_class: &str,
        filters: &[crate::list_query::FilterClause],
    ) -> Result<ResolvedRelatedClassGraph, AppError> {
        let validated = validate_filter_clauses(filters, RELATED_CLASS_FILTER_SPECS)?;
        let class = self.class_handle_by_name(root_class)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let graph = class.related_graph().filters(filters).fetch()?;

        let class_map = self.class_map_from_ids(
            graph
                .classes
                .iter()
                .map(|related_class| related_class.id)
                .chain(graph.relations.iter().flat_map(|relation| {
                    [relation.from_hubuum_class_id, relation.to_hubuum_class_id]
                }))
                .chain(std::iter::once(class.id()))
                .collect::<Vec<_>>(),
        )?;
        let namespace_map = self.namespace_map_from_ids(
            graph
                .classes
                .iter()
                .map(|related_class| related_class.namespace_id)
                .chain(std::iter::once(class.resource().namespace.id))
                .collect::<Vec<_>>(),
        )?;

        Ok(ResolvedRelatedClassGraph {
            classes: graph
                .classes
                .iter()
                .map(|related_class| {
                    ResolvedRelatedClassRecord::new(
                        related_class,
                        &namespace_map,
                        self.related_class_path_labels(&related_class.path, class.id(), &class_map),
                    )
                })
                .collect(),
            relations: graph
                .relations
                .iter()
                .map(|relation| ResolvedClassRelationRecord::new(relation, &class_map))
                .collect(),
        })
    }

    pub fn get_class_relation_by_id(
        &self,
        relation_id: i32,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let relation = self
            .client
            .class_relation()
            .select(relation_id)?
            .resource()
            .clone();
        let class_map =
            self.class_map_from_ids([relation.from_hubuum_class_id, relation.to_hubuum_class_id])?;
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn get_class_relation_by_pair(
        &self,
        class_a: &str,
        class_b: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let classes = self.class_pair(class_a, class_b)?;
        let relation = self.find_class_relation_between(classes.0.id, classes.1.id)?;
        let class_map = self.class_map_from_classes([&classes.0, &classes.1]);
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn delete_class_relation_by_id(&self, relation_id: i32) -> Result<(), AppError> {
        self.client.class_relation().delete(relation_id)?;
        Ok(())
    }

    pub fn delete_class_relation_by_pair(
        &self,
        class_a: &str,
        class_b: &str,
    ) -> Result<(), AppError> {
        let classes = self.class_pair(class_a, class_b)?;
        let relation = self.find_class_relation_between(classes.0.id, classes.1.id)?;
        self.class_handle_by_name(class_a)?
            .delete_relation(relation.id)?;
        Ok(())
    }

    pub fn list_related_object_relations(
        &self,
        root: &RelationRoot,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedObjectRelationRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, OBJECT_RELATION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, OBJECT_RELATION_SORT_SPECS)?;
        let object = self.object_handle_by_name(&root.root_class, &root.root_object)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = apply_cursor_request_paging(
            object.related_relations().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        self.resolve_object_relation_page(page, query.limit)
    }

    pub fn get_object_relation_by_id(
        &self,
        relation_id: i32,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let relation = self
            .client
            .object_relation()
            .select(relation_id)?
            .resource()
            .clone();
        self.resolve_object_relation_record(&relation)
    }

    pub fn get_object_relation_v2(
        &self,
        target: &RelationTarget,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let (object_a, object_b) = self.canonical_object_relation_handles(target)?;
        let relation = object_a.relation_to(object_b.resource().hubuum_class_id, object_b.id())?;
        self.resolve_object_relation_record(relation.resource())
    }

    pub fn create_class_relation_v2(
        &self,
        class_a: &str,
        class_b: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let mut classes = (
            self.class_handle_by_name(class_a)?,
            self.class_handle_by_name(class_b)?,
        );
        if classes.0.id() > classes.1.id() {
            std::mem::swap(&mut classes.0, &mut classes.1);
        }
        let relation = classes.0.create_relation(classes.1.id())?;
        let class_map =
            self.class_map_from_ids([relation.from_hubuum_class_id, relation.to_hubuum_class_id])?;
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn create_object_relation_v2(
        &self,
        target: &RelationTarget,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let (object_a, object_b) = self.canonical_object_relation_handles(target)?;
        let relation =
            object_a.create_relation_to(object_b.resource().hubuum_class_id, object_b.id())?;
        self.resolve_object_relation_record(&relation)
    }

    pub fn delete_object_relation_by_id(&self, relation_id: i32) -> Result<(), AppError> {
        self.client.object_relation().delete(relation_id)?;
        Ok(())
    }

    pub fn delete_object_relation_v2(&self, target: &RelationTarget) -> Result<(), AppError> {
        let (object_a, object_b) = self.canonical_object_relation_handles(target)?;
        object_a.delete_relation_to(object_b.resource().hubuum_class_id, object_b.id())?;
        Ok(())
    }

    pub fn list_related_objects(
        &self,
        root: &RelationRoot,
        options: &RelatedObjectOptions,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedRelatedObjectRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, RELATED_OBJECT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, RELATED_OBJECT_SORT_SPECS)?;
        let object = self.object_handle_by_name(&root.root_class, &root.root_object)?;
        let ignore_classes = options
            .ignore_classes
            .iter()
            .map(|class_name| {
                self.class_handle_by_name(class_name)
                    .map(|class| class.id())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let request = object
            .related_objects()
            .filters(filters)
            .ignore_self_class(!options.include_self_class);
        let request = if ignore_classes.is_empty() {
            request
        } else {
            request.ignore_classes(ignore_classes)
        };
        let page = apply_cursor_request_paging(request, query, &validated_sorts).page()?;
        self.resolve_related_object_page(page, query.limit, object.resource())
    }

    pub fn related_object_graph(
        &self,
        root: &RelationRoot,
        filters: &[crate::list_query::FilterClause],
    ) -> Result<ResolvedRelatedObjectGraph, AppError> {
        let validated = validate_filter_clauses(filters, RELATED_OBJECT_FILTER_SPECS)?;
        let object = self.object_handle_by_name(&root.root_class, &root.root_object)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let graph = object.related_graph().filters(filters).fetch()?;

        let class_map = self.class_map_from_ids(
            graph
                .objects
                .iter()
                .map(|object| object.hubuum_class_id)
                .collect::<Vec<_>>(),
        )?;
        let namespace_map = self.namespace_map_from_ids(
            graph
                .objects
                .iter()
                .map(|object| object.namespace_id)
                .collect::<Vec<_>>(),
        )?;
        let object_map = graph
            .objects
            .iter()
            .map(|object| {
                (
                    object.id,
                    hubuum_client::Object {
                        id: object.id,
                        name: object.name.clone(),
                        namespace_id: object.namespace_id,
                        hubuum_class_id: object.hubuum_class_id,
                        description: object.description.clone(),
                        data: Some(object.data.clone()),
                        created_at: object.created_at.clone(),
                        updated_at: object.updated_at.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let class_relation_map = find_entities_by_ids(
            &self.client.class_relation(),
            graph.relations.iter(),
            |relation| relation.class_relation_id,
        )?;

        Ok(ResolvedRelatedObjectGraph {
            objects: graph
                .objects
                .iter()
                .map(|related_object| {
                    ResolvedRelatedObjectRecord::new(
                        related_object,
                        &class_map,
                        &namespace_map,
                        self.related_object_path_labels(
                            &related_object.path,
                            object.resource().id,
                            &object_map,
                        ),
                    )
                })
                .collect(),
            relations: graph
                .relations
                .iter()
                .filter_map(|relation| {
                    class_relation_map
                        .get(&relation.class_relation_id)
                        .map(|class_relation| {
                            ResolvedObjectRelationRecord::new(
                                relation,
                                class_relation,
                                &object_map,
                                &class_map,
                            )
                        })
                })
                .collect(),
        })
    }

    fn resolve_object_relation_record(
        &self,
        relation: &hubuum_client::ObjectRelation,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let class_relation = self
            .client
            .class_relation()
            .select(relation.class_relation_id)?
            .resource()
            .clone();
        let object_map = self.object_map_for_relation(
            std::slice::from_ref(relation),
            class_relation.from_hubuum_class_id,
            class_relation.to_hubuum_class_id,
        )?;
        let class_map = self.class_map_from_ids([
            class_relation.from_hubuum_class_id,
            class_relation.to_hubuum_class_id,
        ])?;
        Ok(ResolvedObjectRelationRecord::new(
            relation,
            &class_relation,
            &object_map,
            &class_map,
        ))
    }

    fn resolve_object_relation_page(
        &self,
        page: hubuum_client::Page<hubuum_client::ObjectRelation>,
        limit: Option<usize>,
    ) -> Result<PagedResult<ResolvedObjectRelationRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit,
                returned_count: 0,
            });
        }

        let class_relation_map = find_entities_by_ids(
            &self.client.class_relation(),
            page.items.iter(),
            |relation| relation.class_relation_id,
        )?;
        let class_map = self.class_map_from_ids(
            class_relation_map
                .values()
                .flat_map(|relation| [relation.from_hubuum_class_id, relation.to_hubuum_class_id])
                .collect::<Vec<_>>(),
        )?;
        let object_map =
            self.resolve_object_map_from_relations(&page.items, &class_relation_map)?;

        Ok(PagedResult::from_page(page, limit, |relation| {
            let class_relation = class_relation_map
                .get(&relation.class_relation_id)
                .expect("class relation should be loaded");
            ResolvedObjectRelationRecord::new(&relation, class_relation, &object_map, &class_map)
        }))
    }

    fn resolve_object_map_from_relations(
        &self,
        relations: &[hubuum_client::ObjectRelation],
        class_relation_map: &HashMap<i32, hubuum_client::ClassRelation>,
    ) -> Result<HashMap<i32, hubuum_client::Object>, AppError> {
        let mut grouped = HashMap::<i32, Vec<i32>>::new();
        for relation in relations {
            if let Some(class_relation) = class_relation_map.get(&relation.class_relation_id) {
                grouped
                    .entry(class_relation.from_hubuum_class_id)
                    .or_default()
                    .push(relation.from_hubuum_object_id);
                grouped
                    .entry(class_relation.to_hubuum_class_id)
                    .or_default()
                    .push(relation.to_hubuum_object_id);
            }
        }

        let mut objects = HashMap::new();
        for (class_id, object_ids) in grouped {
            let joined = object_ids
                .into_iter()
                .map(|object_id| object_id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            for object in self
                .client
                .objects(class_id)
                .find()
                .add_filter_equals("id", joined.clone())
                .execute()?
            {
                objects.insert(object.id, object);
            }
        }

        Ok(objects)
    }

    fn resolve_related_object_page(
        &self,
        page: hubuum_client::Page<hubuum_client::ObjectWithPath>,
        limit: Option<usize>,
        root_object: &hubuum_client::Object,
    ) -> Result<PagedResult<ResolvedRelatedObjectRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit,
                returned_count: 0,
            });
        }

        let class_map = self.class_map_from_ids(
            page.items
                .iter()
                .map(|object| object.hubuum_class_id)
                .collect::<Vec<_>>(),
        )?;
        let namespace_map = self.namespace_map_from_ids(
            page.items
                .iter()
                .map(|object| object.namespace_id)
                .collect::<Vec<_>>(),
        )?;
        let path_object_map = page
            .items
            .iter()
            .map(|object| {
                (
                    object.id,
                    hubuum_client::Object {
                        id: object.id,
                        name: object.name.clone(),
                        namespace_id: object.namespace_id,
                        hubuum_class_id: object.hubuum_class_id,
                        description: object.description.clone(),
                        data: Some(object.data.clone()),
                        created_at: object.created_at.clone(),
                        updated_at: object.updated_at.clone(),
                    },
                )
            })
            .chain(std::iter::once((root_object.id, root_object.clone())))
            .collect::<HashMap<_, _>>();

        Ok(PagedResult::from_page(page, limit, |object| {
            ResolvedRelatedObjectRecord::new(
                &object,
                &class_map,
                &namespace_map,
                self.related_object_path_labels(&object.path, root_object.id, &path_object_map),
            )
        }))
    }

    fn resolve_related_class_page(
        &self,
        page: hubuum_client::Page<hubuum_client::ClassWithPath>,
        limit: Option<usize>,
        root_class: &hubuum_client::Class,
    ) -> Result<PagedResult<ResolvedRelatedClassRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit,
                returned_count: 0,
            });
        }

        let class_map = self.class_map_from_ids(
            page.items
                .iter()
                .flat_map(|class| class.path.iter().copied().chain(std::iter::once(class.id)))
                .chain(std::iter::once(root_class.id))
                .collect::<Vec<_>>(),
        )?;
        let namespace_map = self.namespace_map_from_ids(
            page.items
                .iter()
                .map(|class| class.namespace_id)
                .chain(std::iter::once(root_class.namespace.id))
                .collect::<Vec<_>>(),
        )?;

        Ok(PagedResult::from_page(page, limit, |class| {
            ResolvedRelatedClassRecord::new(
                &class,
                &namespace_map,
                self.related_class_path_labels(&class.path, root_class.id, &class_map),
            )
        }))
    }

    fn related_class_path_labels(
        &self,
        path: &[i32],
        root_class_id: i32,
        class_map: &HashMap<i32, hubuum_client::Class>,
    ) -> Vec<String> {
        path.iter()
            .copied()
            .filter(|class_id| *class_id != root_class_id)
            .map(|class_id| {
                class_map
                    .get(&class_id)
                    .map(|class| class.name.clone())
                    .unwrap_or_else(|| class_id.to_string())
            })
            .collect()
    }

    fn related_object_path_labels(
        &self,
        path: &[i32],
        root_object_id: i32,
        object_map: &HashMap<i32, hubuum_client::Object>,
    ) -> Vec<String> {
        path.iter()
            .copied()
            .filter(|object_id| *object_id != root_object_id)
            .map(|object_id| {
                object_map
                    .get(&object_id)
                    .map(|object| object.name.clone())
                    .unwrap_or_else(|| object_id.to_string())
            })
            .collect()
    }

    fn canonical_object_relation_handles(
        &self,
        target: &RelationTarget,
    ) -> Result<
        (
            hubuum_client::client::sync::Handle<hubuum_client::Object>,
            hubuum_client::client::sync::Handle<hubuum_client::Object>,
        ),
        AppError,
    > {
        let (object_a_name, object_b_name) = validate_object_names(target)?;
        let class_a = self.class_handle_by_name(&target.class_a)?;
        let class_b = self.class_handle_by_name(&target.class_b)?;
        let object_a = class_a.object_by_name(object_a_name)?;
        let object_b = class_b.object_by_name(object_b_name)?;
        let class_relation = self.find_class_relation_between(class_a.id(), class_b.id())?;

        if class_relation.from_hubuum_class_id == class_a.id() {
            Ok((object_a, object_b))
        } else {
            Ok((object_b, object_a))
        }
    }
}

pub(crate) const CLASS_RELATION_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_a",
        "from_class_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "class_b",
        "to_class_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "class_a_id",
        "from_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_b_id",
        "to_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
];

pub(crate) const CLASS_RELATION_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("class_a_id", "from_classes"),
    SortFieldSpec::new("class_b_id", "to_classes"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

pub(crate) const RELATED_CLASS_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "namespace_id",
        "namespace_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "namespaces",
        "namespace_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "classes",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "from_classes",
        "from_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_classes",
        "to_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "from_namespaces",
        "from_namespaces",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_namespaces",
        "to_namespaces",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "from_name",
        "from_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "to_name",
        "to_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "from_description",
        "from_description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "to_description",
        "to_description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "from_created_at",
        "from_created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "to_created_at",
        "to_created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "from_updated_at",
        "from_updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "to_updated_at",
        "to_updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "depth",
        "depth",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "path",
        "path",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    ),
];

pub(crate) const RELATED_CLASS_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("namespace_id", "namespace_id"),
    SortFieldSpec::new("namespaces", "namespace_id"),
    SortFieldSpec::new("class_id", "id"),
    SortFieldSpec::new("classes", "id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
    SortFieldSpec::new("from_classes", "from_classes"),
    SortFieldSpec::new("to_classes", "to_classes"),
    SortFieldSpec::new("from_namespaces", "from_namespaces"),
    SortFieldSpec::new("to_namespaces", "to_namespaces"),
    SortFieldSpec::new("from_name", "from_name"),
    SortFieldSpec::new("to_name", "to_name"),
    SortFieldSpec::new("from_description", "from_description"),
    SortFieldSpec::new("to_description", "to_description"),
    SortFieldSpec::new("from_created_at", "from_created_at"),
    SortFieldSpec::new("to_created_at", "to_created_at"),
    SortFieldSpec::new("from_updated_at", "from_updated_at"),
    SortFieldSpec::new("to_updated_at", "to_updated_at"),
    SortFieldSpec::new("depth", "depth"),
    SortFieldSpec::new("path", "path"),
];

pub(crate) const OBJECT_RELATION_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_relation_id",
        "class_relation",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "object_a_id",
        "from_objects",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "object_b_id",
        "to_objects",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
];

pub(crate) const OBJECT_RELATION_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("class_relation_id", "class_relation"),
    SortFieldSpec::new("object_a_id", "from_objects"),
    SortFieldSpec::new("object_b_id", "to_objects"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

pub(crate) const RELATED_OBJECT_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "namespace_id",
        "namespace_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "class_id",
        "class_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "from_object_id",
        "from_objects",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_object_id",
        "to_objects",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "from_class_id",
        "from_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_class_id",
        "to_classes",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "from_namespace_id",
        "from_namespaces",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_namespace_id",
        "to_namespaces",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "from_name",
        "from_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "to_name",
        "to_name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "from_description",
        "from_description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "to_description",
        "to_description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "from_created_at",
        "from_created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "to_created_at",
        "to_created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "from_updated_at",
        "from_updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "to_updated_at",
        "to_updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "from_json_data",
        "from_json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
    FilterFieldSpec::new(
        "to_json_data",
        "to_json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
    FilterFieldSpec::new(
        "depth",
        "depth",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "path",
        "path",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    ),
];

pub(crate) const RELATED_OBJECT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("namespace_id", "namespace_id"),
    SortFieldSpec::new("class_id", "class_id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
    SortFieldSpec::new("from_object_id", "from_objects"),
    SortFieldSpec::new("to_object_id", "to_objects"),
    SortFieldSpec::new("from_class_id", "from_classes"),
    SortFieldSpec::new("to_class_id", "to_classes"),
    SortFieldSpec::new("from_namespace_id", "from_namespaces"),
    SortFieldSpec::new("to_namespace_id", "to_namespaces"),
    SortFieldSpec::new("from_name", "from_name"),
    SortFieldSpec::new("to_name", "to_name"),
    SortFieldSpec::new("from_description", "from_description"),
    SortFieldSpec::new("to_description", "to_description"),
    SortFieldSpec::new("from_created_at", "from_created_at"),
    SortFieldSpec::new("to_created_at", "to_created_at"),
    SortFieldSpec::new("from_updated_at", "from_updated_at"),
    SortFieldSpec::new("to_updated_at", "to_updated_at"),
    SortFieldSpec::new("depth", "depth"),
    SortFieldSpec::new("path", "path"),
];

fn validate_object_names(target: &RelationTarget) -> Result<(&str, &str), AppError> {
    match (target.object_a.as_deref(), target.object_b.as_deref()) {
        (Some(object_a), Some(object_b)) => Ok((object_a, object_b)),
        (None, _) => Err(AppError::MissingOptions(vec!["object-a".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object-b".to_string()])),
    }
}
