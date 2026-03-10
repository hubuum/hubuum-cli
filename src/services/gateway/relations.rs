use std::collections::HashMap;

use hubuum_client::{ClassRelationPost, ObjectRelationPost};

use crate::domain::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct RelationTarget {
    pub class_from: String,
    pub class_to: String,
    pub object_from: Option<String>,
    pub object_to: Option<String>,
}

impl HubuumGateway {
    pub fn create_class_relation(
        &self,
        class_from: &str,
        class_to: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let (class_from, class_to) = self.class_pair(class_from, class_to)?;
        let relation = self.client.class_relation().create_raw(ClassRelationPost {
            from_hubuum_class_id: class_from.id,
            to_hubuum_class_id: class_to.id,
        })?;

        let class_map = self.class_map_from_classes([&class_from, &class_to]);
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn create_object_relation(
        &self,
        target: &RelationTarget,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let (object_from_name, object_to_name) = validate_object_names(target)?;
        let (class_from, class_to) = self.class_pair(&target.class_from, &target.class_to)?;
        let class_relation = self.find_class_relation(class_from.id, class_to.id)?;
        let object_from = self.find_object_by_name(class_from.id, object_from_name)?;
        let object_to = self.find_object_by_name(class_to.id, object_to_name)?;

        let relation = self
            .client
            .object_relation()
            .create_raw(ObjectRelationPost {
                class_relation_id: class_relation.id,
                from_hubuum_object_id: object_from.id,
                to_hubuum_object_id: object_to.id,
            })?;

        let class_map = self.class_map_from_classes([&class_from, &class_to]);
        let object_map = HashMap::from([(object_from.id, object_from), (object_to.id, object_to)]);

        Ok(ResolvedObjectRelationRecord::new(
            &relation,
            &class_relation,
            &object_map,
            &class_map,
        ))
    }

    pub fn delete_class_relation(&self, class_from: &str, class_to: &str) -> Result<(), AppError> {
        let (class_from, class_to) = self.class_pair(class_from, class_to)?;
        let relation = self.find_class_relation(class_from.id, class_to.id)?;
        self.client.class_relation().delete(relation.id)?;
        Ok(())
    }

    pub fn delete_object_relation(&self, target: &RelationTarget) -> Result<(), AppError> {
        let (object_from_name, object_to_name) = validate_object_names(target)?;
        let (class_from, class_to) = self.class_pair(&target.class_from, &target.class_to)?;
        let class_relation = self.find_class_relation(class_from.id, class_to.id)?;
        let object_from = self.find_object_by_name(class_from.id, object_from_name)?;
        let object_to = self.find_object_by_name(class_to.id, object_to_name)?;
        let relation = self.find_object_relation(&class_relation, &object_from, &object_to)?;
        self.client.object_relation().delete(relation.id)?;
        Ok(())
    }

    pub fn get_class_relation(
        &self,
        class_from: &str,
        class_to: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let (class_from, class_to) = self.class_pair(class_from, class_to)?;
        let relation = self.find_class_relation(class_from.id, class_to.id)?;
        let class_map = self.class_map_from_classes([&class_from, &class_to]);
        Ok(ResolvedClassRelationRecord::new(&relation, &class_map))
    }

    pub fn get_object_relation(
        &self,
        target: &RelationTarget,
    ) -> Result<ResolvedObjectRelationRecord, AppError> {
        let (object_from_name, object_to_name) = validate_object_names(target)?;
        let (class_from, class_to) = self.class_pair(&target.class_from, &target.class_to)?;
        let object_from = self.find_object_by_name(class_from.id, object_from_name)?;
        let object_to = self.find_object_by_name(class_to.id, object_to_name)?;
        let class_relation = self.find_class_relation(class_from.id, class_to.id)?;
        let object_relation =
            self.find_object_relation(&class_relation, &object_from, &object_to)?;

        let class_map = self.class_map_from_classes([&class_from, &class_to]);
        let object_map = HashMap::from([(object_from.id, object_from), (object_to.id, object_to)]);

        Ok(ResolvedObjectRelationRecord::new(
            &object_relation,
            &class_relation,
            &object_map,
            &class_map,
        ))
    }

    pub fn list_class_relations(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedClassRelationRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, RELATION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, RELATION_SORT_SPECS)?;
        if validated
            .iter()
            .any(|clause| matches!(clause.spec.public_name, "object_from" | "object_to"))
        {
            return Err(AppError::ParseError(
                "object_from/object_to filters require object relation listing".to_string(),
            ));
        }

        let mut search = self.client.class_relation().find();
        if let Some(class_from_name) = relation_filter_value(&validated, "class_from") {
            let class_from = self.client.classes().select_by_name(class_from_name)?;
            search = search.add_filter_equals("from_classes", class_from.id());
        }
        if let Some(class_to_name) = relation_filter_value(&validated, "class_to") {
            let class_to = self.client.classes().select_by_name(class_to_name)?;
            search = search.add_filter_equals("to_classes", class_to.id());
        }

        let page = apply_query_paging(search, query, &validated_sorts).page()?;
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

    pub fn list_object_relations(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ResolvedObjectRelationRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, RELATION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, RELATION_SORT_SPECS)?;
        let class_from_name = relation_filter_value(&validated, "class_from")
            .ok_or_else(|| AppError::MissingOptions(vec!["class_from".to_string()]))?;
        let class_to_name = relation_filter_value(&validated, "class_to")
            .ok_or_else(|| AppError::MissingOptions(vec!["class_to".to_string()]))?;

        let (mut class_from, mut class_to) = self.class_pair(class_from_name, class_to_name)?;
        let mut swapped = false;
        if class_from.id > class_to.id {
            swapped = true;
            std::mem::swap(&mut class_from, &mut class_to);
        }

        let class_relation = self.find_class_relation(class_from.id, class_to.id)?;
        let mut search = self
            .client
            .object_relation()
            .find()
            .add_filter_equals("class_relation", class_relation.id);

        if let Some(object_from_name) = relation_filter_value(&validated, "object_from") {
            let object_from = self.find_object_by_name(class_from.id, object_from_name)?;
            let target = if swapped {
                "from_objects"
            } else {
                "to_objects"
            };
            search = search.add_filter_equals(target, object_from.id);
        }

        if let Some(object_to_name) = relation_filter_value(&validated, "object_to") {
            let object_to = self.find_object_by_name(class_to.id, object_to_name)?;
            let target = if swapped {
                "to_objects"
            } else {
                "from_objects"
            };
            search = search.add_filter_equals(target, object_to.id);
        }

        let page = apply_query_paging(search, query, &validated_sorts).page()?;
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit: query.limit,
                returned_count: 0,
            });
        }

        let object_map = self.object_map_for_relation(&page.items, class_from.id, class_to.id)?;
        let class_map = self.class_map_from_classes([&class_from, &class_to]);

        let mut corrected_relation = class_relation.clone();
        if swapped {
            std::mem::swap(
                &mut corrected_relation.from_hubuum_class_id,
                &mut corrected_relation.to_hubuum_class_id,
            );
        }

        Ok(PagedResult::from_page(page, query.limit, |relation| {
            ResolvedObjectRelationRecord::new(
                &relation,
                &corrected_relation,
                &object_map,
                &class_map,
            )
        }))
    }
}

pub(crate) const RELATION_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "class_from",
        "from_classes",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "class_to",
        "to_classes",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "object_from",
        "from_objects",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "object_to",
        "to_objects",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
];

pub(crate) const RELATION_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("class_from", "from_classes"),
    SortFieldSpec::new("class_to", "to_classes"),
    SortFieldSpec::new("object_from", "from_objects"),
    SortFieldSpec::new("object_to", "to_objects"),
];

fn validate_object_names(target: &RelationTarget) -> Result<(&str, &str), AppError> {
    match (target.object_from.as_deref(), target.object_to.as_deref()) {
        (Some(from), Some(to)) => Ok((from, to)),
        (None, _) => Err(AppError::MissingOptions(vec!["object_from".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object_to".to_string()])),
    }
}

fn relation_filter_value<'a>(
    clauses: &'a [crate::list_query::ValidatedFilterClause],
    field: &str,
) -> Option<&'a str> {
    clauses
        .iter()
        .find(|clause| clause.spec.public_name == field)
        .map(|clause| clause.value.as_str())
}
