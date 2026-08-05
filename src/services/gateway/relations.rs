use std::collections::HashMap;
use std::iter::once;
use std::mem::swap;
use std::slice::from_ref;

use hubuum_client::{
    client::sync::Handle as SyncHandle, Class, ClassRelation, ClassRelationCreateOptions,
    ClassWithPath, Object, ObjectRelation, ObjectRelationLimit, ObjectWithPath,
};

use crate::domain::{
    ResolvedClassRelationRecord, ResolvedObjectRelationRecord, ResolvedRelatedClassGraph,
    ResolvedRelatedClassRecord, ResolvedRelatedObjectGraph, ResolvedRelatedObjectRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, validate_filter_clauses, validate_sort_clauses, FilterClause,
    FilterFieldSpec, FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult,
    SortFieldSpec,
};

use super::{
    shared::{fetch_entities_for_ids, find_entities_by_ids},
    HubuumGateway,
};

#[derive(Debug, Clone)]
pub struct RelationTarget {
    pub class_a: String,
    pub class_b: String,
    pub object_a: Option<String>,
    pub object_b: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateClassRelationInput {
    class_a: ClassRelationEndpointInput,
    class_b: ClassRelationEndpointInput,
}

#[derive(Debug, Clone)]
struct ClassRelationEndpointInput {
    class_name: String,
    template_alias: Option<String>,
    max_relations: Option<ObjectRelationLimit>,
}

impl ClassRelationEndpointInput {
    fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_name: class_name.into(),
            template_alias: None,
            max_relations: None,
        }
    }
}

impl CreateClassRelationInput {
    pub fn new(class_a: impl Into<String>, class_b: impl Into<String>) -> Self {
        Self {
            class_a: ClassRelationEndpointInput::new(class_a),
            class_b: ClassRelationEndpointInput::new(class_b),
        }
    }

    pub fn with_forward_template_alias(mut self, alias: impl Into<String>) -> Self {
        self.class_a.template_alias = Some(alias.into());
        self
    }

    pub fn with_reverse_template_alias(mut self, alias: impl Into<String>) -> Self {
        self.class_b.template_alias = Some(alias.into());
        self
    }

    pub fn with_from_max_relations(mut self, limit: ObjectRelationLimit) -> Self {
        self.class_a.max_relations = Some(limit);
        self
    }

    pub fn with_to_max_relations(mut self, limit: ObjectRelationLimit) -> Self {
        self.class_b.max_relations = Some(limit);
        self
    }

    fn class_names(&self) -> (&str, &str) {
        (&self.class_a.class_name, &self.class_b.class_name)
    }

    fn reverse_direction(&mut self) {
        swap(&mut self.class_a, &mut self.class_b);
    }

    fn into_client_options(self) -> ClassRelationCreateOptions {
        let mut options = ClassRelationCreateOptions::default();
        if let Some(alias) = self.class_a.template_alias {
            options = options.with_forward_template_alias(alias);
        }
        if let Some(alias) = self.class_b.template_alias {
            options = options.with_reverse_template_alias(alias);
        }
        if let Some(limit) = self.class_a.max_relations {
            options = options.with_from_max_relations(limit);
        }
        if let Some(limit) = self.class_b.max_relations {
            options = options.with_to_max_relations(limit);
        }
        options
    }
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
        let page = fetch_cursor_results(
            class.related_classes().filters(filters),
            query,
            &validated_sorts,
        )?;

        self.resolve_related_class_page(page, class.resource())
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
        let page = fetch_cursor_results(
            class.related_relations().filters(filters),
            query,
            &validated_sorts,
        )?;
        if page.items.is_empty() {
            return Ok(PagedResult::empty(page.next_cursor, page.total_count));
        }

        let class_map = self.class_map_from_relation_ids(&page.items)?;
        Ok(page.map(|relation| ResolvedClassRelationRecord::new(&relation, &class_map)))
    }

    pub fn related_class_graph(
        &self,
        root_class: &str,
        filters: &[FilterClause],
    ) -> Result<ResolvedRelatedClassGraph, AppError> {
        let validated = validate_filter_clauses(filters, RELATED_CLASS_FILTER_SPECS)?;
        let class = self.class_handle_by_name(root_class)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let graph = class.related_graph().filters(filters).send()?;

        let class_map = self.class_map_from_ids(
            graph
                .classes
                .iter()
                .map(|related_class| related_class.id)
                .chain(graph.relations.iter().flat_map(|relation| {
                    [relation.from_hubuum_class_id, relation.to_hubuum_class_id]
                }))
                .chain(once(class.id()))
                .collect::<Vec<_>>(),
        )?;
        let collection_map = self.collection_map_from_ids(
            graph
                .classes
                .iter()
                .map(|related_class| related_class.collection_id)
                .chain(once(class.resource().collection.id))
                .collect::<Vec<_>>(),
        )?;

        Ok(ResolvedRelatedClassGraph {
            classes: graph
                .classes
                .iter()
                .map(|related_class| {
                    ResolvedRelatedClassRecord::new(
                        related_class,
                        &collection_map,
                        self.related_class_path_labels(
                            &related_class.path,
                            class.id().into(),
                            &class_map,
                        ),
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

    pub fn get_class_relation_by_pair(
        &self,
        class_a: &str,
        class_b: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let classes = self.class_pair(class_a, class_b)?;
        let relation =
            self.find_class_relation_between(classes.0.id.into(), classes.1.id.into())?;
        let class_map = self.class_map_from_classes([&classes.0, &classes.1]);
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn delete_class_relation_by_pair(
        &self,
        class_a: &str,
        class_b: &str,
    ) -> Result<(), AppError> {
        let classes = self.class_pair(class_a, class_b)?;
        let relation =
            self.find_class_relation_between(classes.0.id.into(), classes.1.id.into())?;
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
        let page = fetch_cursor_results(
            object.related_relations().filters(filters),
            query,
            &validated_sorts,
        )?;
        self.resolve_object_relation_page(page)
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
        mut input: CreateClassRelationInput,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let (class_a, class_b) = input.class_names();
        let mut classes = (
            self.class_handle_by_name(class_a)?,
            self.class_handle_by_name(class_b)?,
        );
        let class_a_id: i32 = classes.0.id().into();
        let class_b_id: i32 = classes.1.id().into();
        if class_a_id > class_b_id {
            swap(&mut classes.0, &mut classes.1);
            input.reverse_direction();
        }

        let relation = classes
            .0
            .create_relation_with_options(classes.1.id(), input.into_client_options())?;
        let class_map = self.class_map_from_classes([classes.0.resource(), classes.1.resource()]);
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
                    .map(|class| class.id().into())
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
        let page = fetch_cursor_results(request, query, &validated_sorts)?;
        self.resolve_related_object_page(page, object.resource())
    }

    pub fn related_object_graph(
        &self,
        root: &RelationRoot,
        filters: &[FilterClause],
    ) -> Result<ResolvedRelatedObjectGraph, AppError> {
        let validated = validate_filter_clauses(filters, RELATED_OBJECT_FILTER_SPECS)?;
        let object = self.object_handle_by_name(&root.root_class, &root.root_object)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let graph = object.related_graph().filters(filters).send()?;

        let class_map = self.class_map_from_ids(
            graph
                .objects
                .iter()
                .map(|object| object.hubuum_class_id)
                .collect::<Vec<_>>(),
        )?;
        let collection_map = self.collection_map_from_ids(
            graph
                .objects
                .iter()
                .map(|object| object.collection_id)
                .collect::<Vec<_>>(),
        )?;
        let object_map = graph
            .objects
            .iter()
            .map(|object| Ok((i32::from(object.id), object_from_path(object)?)))
            .collect::<Result<HashMap<_, _>, AppError>>()?;
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
                        &collection_map,
                        self.related_object_path_labels(
                            &related_object.path,
                            object.resource().id.into(),
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
                        .get(&relation.class_relation_id.into())
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
        relation: &ObjectRelation,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let class_relation = self
            .client
            .class_relation()
            .get(relation.class_relation_id)?
            .resource()
            .clone();
        let object_map = self.object_map_for_relation(
            from_ref(relation),
            class_relation.from_hubuum_class_id.into(),
            class_relation.to_hubuum_class_id.into(),
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
        page: PagedResult<ObjectRelation>,
    ) -> Result<PagedResult<ResolvedObjectRelationRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult::empty(page.next_cursor, page.total_count));
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

        Ok(page.map(|relation| {
            let class_relation = class_relation_map
                .get(&relation.class_relation_id.into())
                .expect("class relation should be loaded");
            ResolvedObjectRelationRecord::new(&relation, class_relation, &object_map, &class_map)
        }))
    }

    fn resolve_object_map_from_relations(
        &self,
        relations: &[ObjectRelation],
        class_relation_map: &HashMap<i32, ClassRelation>,
    ) -> Result<HashMap<i32, Object>, AppError> {
        let mut grouped = HashMap::<i32, Vec<i32>>::new();
        for relation in relations {
            if let Some(class_relation) = class_relation_map.get(&relation.class_relation_id.into())
            {
                grouped
                    .entry(class_relation.from_hubuum_class_id.into())
                    .or_default()
                    .push(relation.from_hubuum_object_id.into());
                grouped
                    .entry(class_relation.to_hubuum_class_id.into())
                    .or_default()
                    .push(relation.to_hubuum_object_id.into());
            }
        }

        let mut objects = HashMap::new();
        for (class_id, object_ids) in grouped {
            objects.extend(fetch_entities_for_ids(
                &self.client.objects(class_id),
                object_ids,
            )?);
        }

        Ok(objects)
    }

    fn resolve_related_object_page(
        &self,
        page: PagedResult<ObjectWithPath>,
        root_object: &Object,
    ) -> Result<PagedResult<ResolvedRelatedObjectRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult::empty(page.next_cursor, page.total_count));
        }

        let class_map = self.class_map_from_ids(
            page.items
                .iter()
                .map(|object| object.hubuum_class_id)
                .collect::<Vec<_>>(),
        )?;
        let collection_map = self.collection_map_from_ids(
            page.items
                .iter()
                .map(|object| object.collection_id)
                .collect::<Vec<_>>(),
        )?;
        let path_object_map = page
            .items
            .iter()
            .map(|object| Ok((i32::from(object.id), object_from_path(object)?)))
            .collect::<Result<HashMap<_, _>, AppError>>()?
            .into_iter()
            .chain(once((root_object.id.into(), root_object.clone())))
            .collect::<HashMap<_, _>>();

        Ok(page.map(|object| {
            ResolvedRelatedObjectRecord::new(
                &object,
                &class_map,
                &collection_map,
                self.related_object_path_labels(
                    &object.path,
                    root_object.id.into(),
                    &path_object_map,
                ),
            )
        }))
    }

    fn resolve_related_class_page(
        &self,
        page: PagedResult<ClassWithPath>,
        root_class: &Class,
    ) -> Result<PagedResult<ResolvedRelatedClassRecord>, AppError> {
        if page.items.is_empty() {
            return Ok(PagedResult::empty(page.next_cursor, page.total_count));
        }

        let class_map = self.class_map_from_ids(
            page.items
                .iter()
                .flat_map(|class| class.path.iter().copied().chain(once(class.id)))
                .chain(once(root_class.id))
                .collect::<Vec<_>>(),
        )?;
        let collection_map = self.collection_map_from_ids(
            page.items
                .iter()
                .map(|class| class.collection_id)
                .chain(once(root_class.collection.id))
                .collect::<Vec<_>>(),
        )?;

        Ok(page.map(|class| {
            ResolvedRelatedClassRecord::new(
                &class,
                &collection_map,
                self.related_class_path_labels(&class.path, root_class.id.into(), &class_map),
            )
        }))
    }

    fn related_class_path_labels<Id>(
        &self,
        path: &[Id],
        root_class_id: i32,
        class_map: &HashMap<i32, Class>,
    ) -> Vec<String>
    where
        Id: Copy + Into<i32>,
    {
        path.iter()
            .copied()
            .map(Into::into)
            .filter(|class_id| *class_id != root_class_id)
            .map(|class_id| {
                class_map
                    .get(&class_id)
                    .map(|class| class.name.clone())
                    .unwrap_or_else(|| class_id.to_string())
            })
            .collect()
    }

    fn related_object_path_labels<Id>(
        &self,
        path: &[Id],
        root_object_id: i32,
        object_map: &HashMap<i32, Object>,
    ) -> Vec<String>
    where
        Id: Copy + Into<i32>,
    {
        path.iter()
            .copied()
            .map(Into::into)
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
    ) -> Result<(SyncHandle<Object>, SyncHandle<Object>), AppError> {
        let (object_a_name, object_b_name) = validate_object_names(target)?;
        let class_a = self.class_handle_by_name(&target.class_a)?;
        let class_b = self.class_handle_by_name(&target.class_b)?;
        let object_a = class_a.object_by_name(object_a_name)?;
        let object_b = class_b.object_by_name(object_b_name)?;
        let class_a_id: i32 = class_a.id().into();
        let class_b_id: i32 = class_b.id().into();
        let class_relation = self.find_class_relation_between(class_a_id, class_b_id)?;

        if class_relation.from_hubuum_class_id == class_a_id {
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
        "collection_id",
        "collection_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "collections",
        "collection_id",
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
        "from_collections",
        "from_collections",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_collections",
        "to_collections",
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
    SortFieldSpec::new("collection_id", "collection_id"),
    SortFieldSpec::new("collections", "collection_id"),
    SortFieldSpec::new("class_id", "id"),
    SortFieldSpec::new("classes", "id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
    SortFieldSpec::new("from_classes", "from_classes"),
    SortFieldSpec::new("to_classes", "to_classes"),
    SortFieldSpec::new("from_collections", "from_collections"),
    SortFieldSpec::new("to_collections", "to_collections"),
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
        "collection_id",
        "collection_id",
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
        "from_collection_id",
        "from_collections",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "to_collection_id",
        "to_collections",
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
    SortFieldSpec::new("collection_id", "collection_id"),
    SortFieldSpec::new("class_id", "class_id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
    SortFieldSpec::new("from_object_id", "from_objects"),
    SortFieldSpec::new("to_object_id", "to_objects"),
    SortFieldSpec::new("from_class_id", "from_classes"),
    SortFieldSpec::new("to_class_id", "to_classes"),
    SortFieldSpec::new("from_collection_id", "from_collections"),
    SortFieldSpec::new("to_collection_id", "to_collections"),
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

fn object_from_path(object: &ObjectWithPath) -> Result<Object, AppError> {
    Ok(serde_json::from_value(serde_json::to_value(object)?)?)
}

fn validate_object_names(target: &RelationTarget) -> Result<(&str, &str), AppError> {
    match (target.object_a.as_deref(), target.object_b.as_deref()) {
        (Some(object_a), Some(object_b)) => Ok((object_a, object_b)),
        (None, _) => Err(AppError::MissingOptions(vec!["object-a".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object-b".to_string()])),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use hubuum_client::{
        blocking::Client, ClassRelation, MockTransport, ObjectRelation, ObjectRelationLimit, Token,
        TransportResponse,
    };
    use reqwest::{
        header::{HeaderName, HeaderValue},
        Method, StatusCode,
    };
    use serde_json::{from_slice, from_value, json, Value};

    use super::{CreateClassRelationInput, HubuumGateway};

    #[test]
    fn class_relation_create_preserves_input_side_options_when_ids_are_canonicalized() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &class_json(9, "Hosts"))
                .expect("class A response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &class_json(3, "Rooms"))
                .expect("class B response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(
                StatusCode::CREATED,
                &json!({
                    "id": 7,
                    "from_hubuum_class_id": 3,
                    "to_hubuum_class_id": 9,
                    "forward_template_alias": "hosts",
                    "reverse_template_alias": "rooms",
                    "from_max_relations": 2,
                    "to_max_relations": 1,
                    "created_at": "2026-08-05T12:00:00Z",
                    "updated_at": "2026-08-05T12:00:00Z"
                }),
            )
            .expect("relation response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should be valid")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let relation = gateway
            .create_class_relation_v2(
                CreateClassRelationInput::new("Hosts", "Rooms")
                    .with_forward_template_alias("rooms")
                    .with_reverse_template_alias("hosts")
                    .with_from_max_relations(
                        ObjectRelationLimit::new(1).expect("positive limit should be valid"),
                    )
                    .with_to_max_relations(
                        ObjectRelationLimit::new(2).expect("positive limit should be valid"),
                    ),
            )
            .expect("class relation should be created");

        assert_eq!(relation.class_a, "Rooms");
        assert_eq!(relation.class_b, "Hosts");
        assert_eq!(relation.from_max_relations, Some(2));
        assert_eq!(relation.to_max_relations, Some(1));
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2].method, Method::POST);
        assert_eq!(requests[2].url.path(), "/api/v1/classes/3/relations");
        let body: Value =
            from_slice(requests[2].body()).expect("relation request body should be JSON");
        assert_eq!(body["to_hubuum_class_id"], 9);
        assert_eq!(body["forward_template_alias"], "hosts");
        assert_eq!(body["reverse_template_alias"], "rooms");
        assert_eq!(body["from_max_relations"], 2);
        assert_eq!(body["to_max_relations"], 1);
    }

    #[test]
    fn object_relation_resolution_chunks_filters_and_follows_pages() {
        let transport = MockTransport::default();
        let mut first_page = TransportResponse::json(
            StatusCode::OK,
            &json!([object_json(1, "First resolved object")]),
        )
        .expect("first object lookup page should serialize");
        first_page.headers.insert(
            HeaderName::from_static("x-next-cursor"),
            HeaderValue::from_static("lookup-page-2"),
        );
        transport.push_response(first_page);
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([object_json(101, "Second resolved object")]),
            )
            .expect("second object lookup page should serialize"),
        );
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &json!([]))
                .expect("second object lookup chunk should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should be valid")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let class_relation: ClassRelation = from_value(json!({
            "id": 7,
            "from_hubuum_class_id": 42,
            "to_hubuum_class_id": 42,
            "forward_template_alias": null,
            "reverse_template_alias": null,
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        }))
        .expect("class relation should deserialize");
        let class_relation_map = HashMap::from([(7, class_relation)]);
        let relations = (1..=26)
            .map(|id| {
                from_value::<ObjectRelation>(json!({
                    "id": id,
                    "from_hubuum_object_id": id,
                    "to_hubuum_object_id": id + 100,
                    "class_relation_id": 7,
                    "created_at": "2026-07-25T12:00:00Z",
                    "updated_at": "2026-07-25T12:00:00Z"
                }))
                .expect("object relation should deserialize")
            })
            .collect::<Vec<_>>();

        let objects = gateway
            .resolve_object_map_from_relations(&relations, &class_relation_map)
            .expect("all relation object names should resolve");

        assert_eq!(
            objects.get(&1).map(|object| object.name.as_str()),
            Some("First resolved object")
        );
        assert_eq!(
            objects.get(&101).map(|object| object.name.as_str()),
            Some("Second resolved object")
        );
        let requests = transport.requests();
        assert_eq!(requests.len(), 3);
        assert!(requests[1]
            .url
            .query_pairs()
            .any(|(key, value)| key == "cursor" && value == "lookup-page-2"));
        assert!(requests.iter().all(|request| {
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == "id__equals")
                .is_some_and(|(_, value)| value.split(',').count() <= 50)
        }));
    }

    fn object_json(id: i32, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": name,
            "collection_id": 1,
            "hubuum_class_id": 42,
            "description": "",
            "data": null,
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        })
    }

    fn class_json(id: i32, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": name,
            "description": "",
            "collection": {
                "id": 1,
                "name": "default",
                "description": "",
                "parent_collection_id": null,
                "created_at": "2026-08-05T12:00:00Z",
                "updated_at": "2026-08-05T12:00:00Z"
            },
            "json_schema": null,
            "validate_schema": false,
            "created_at": "2026-08-05T12:00:00Z",
            "updated_at": "2026-08-05T12:00:00Z"
        })
    }
}
