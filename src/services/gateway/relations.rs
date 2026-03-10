use std::collections::HashMap;

use hubuum_client::{ClassRelationPost, ObjectRelationPost};

use crate::domain::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord};
use crate::errors::AppError;

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct RelationTarget {
    pub class_from: String,
    pub class_to: String,
    pub object_from: Option<String>,
    pub object_to: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RelationFilter {
    pub class_from: Option<String>,
    pub class_to: Option<String>,
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
        filter: &RelationFilter,
    ) -> Result<Vec<ResolvedClassRelationRecord>, AppError> {
        let mut query = self.client.class_relation().find();

        if let (Some(class_from_name), Some(class_to_name)) = (&filter.class_from, &filter.class_to)
        {
            let (class_from, class_to) = self.class_pair(class_from_name, class_to_name)?;
            query = query
                .add_filter_equals("from_classes", class_from.id)
                .add_filter_equals("to_classes", class_to.id);
        } else if let Some(class_from_name) = &filter.class_from {
            let class_from = self.client.classes().select_by_name(class_from_name)?;
            query = query.add_filter_equals("from_classes", class_from.id());
        } else if let Some(class_to_name) = &filter.class_to {
            let class_to = self.client.classes().select_by_name(class_to_name)?;
            query = query.add_filter_equals("to_classes", class_to.id());
        }

        let relations = query.execute()?;
        if relations.is_empty() {
            return Ok(Vec::new());
        }

        let class_map = self.class_map_from_relation_ids(&relations)?;
        Ok(relations
            .iter()
            .map(|relation| ResolvedClassRelationRecord::new(relation, &class_map))
            .collect())
    }

    pub fn list_object_relations(
        &self,
        filter: &RelationFilter,
    ) -> Result<Vec<ResolvedObjectRelationRecord>, AppError> {
        let class_from_name = filter
            .class_from
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["class_from".to_string()]))?;
        let class_to_name = filter
            .class_to
            .as_deref()
            .ok_or_else(|| AppError::MissingOptions(vec!["class_to".to_string()]))?;

        let (mut class_from, mut class_to) = self.class_pair(class_from_name, class_to_name)?;
        let mut swapped = false;
        if class_from.id > class_to.id {
            swapped = true;
            std::mem::swap(&mut class_from, &mut class_to);
        }

        let class_relation = self.find_class_relation(class_from.id, class_to.id)?;
        let mut query = self
            .client
            .object_relation()
            .find()
            .add_filter_equals("class_relation", class_relation.id);

        if let Some(object_from_name) = &filter.object_from {
            let object_from = self.find_object_by_name(class_from.id, object_from_name)?;
            let target = if swapped {
                "from_objects"
            } else {
                "to_objects"
            };
            query = query.add_filter_equals(target, object_from.id);
        }

        if let Some(object_to_name) = &filter.object_to {
            let object_to = self.find_object_by_name(class_to.id, object_to_name)?;
            let target = if swapped {
                "to_objects"
            } else {
                "from_objects"
            };
            query = query.add_filter_equals(target, object_to.id);
        }

        let object_relations = query.execute()?;
        if object_relations.is_empty() {
            return Ok(Vec::new());
        }

        let object_map =
            self.object_map_for_relation(&object_relations, class_from.id, class_to.id)?;
        let class_map = self.class_map_from_classes([&class_from, &class_to]);

        let mut corrected_relation = class_relation.clone();
        if swapped {
            std::mem::swap(
                &mut corrected_relation.from_hubuum_class_id,
                &mut corrected_relation.to_hubuum_class_id,
            );
        }

        Ok(object_relations
            .iter()
            .map(|relation| {
                ResolvedObjectRelationRecord::new(
                    relation,
                    &corrected_relation,
                    &object_map,
                    &class_map,
                )
            })
            .collect())
    }
}

fn validate_object_names(target: &RelationTarget) -> Result<(&str, &str), AppError> {
    match (target.object_from.as_deref(), target.object_to.as_deref()) {
        (Some(from), Some(to)) => Ok((from, to)),
        (None, _) => Err(AppError::MissingOptions(vec!["object_from".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object_to".to_string()])),
    }
}
