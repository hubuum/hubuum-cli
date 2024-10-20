use hubuum_client::{ClassRelation, Object, ObjectRelation};

use super::{append_key_value, OutputFormatterWithPadding};
use crate::errors::AppError;

use std::collections::HashMap;

use hubuum_client::{resources::tabled_display, Class};
use tabled::Tabled;

// A wrapper for classrelations that can be outputted where class_ids are replaced with their names
#[derive(Debug, Tabled)]
pub struct FormattedClassRelation {
    pub id: i32,
    #[tabled(rename = "FromClass")]
    pub from_class: String,
    #[tabled(rename = "ToClass")]
    pub to_class: String,
    #[tabled(display_with = "tabled_display", rename = "Created")]
    pub created_at: chrono::NaiveDateTime,
    #[tabled(display_with = "tabled_display", rename = "Updated")]
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Tabled)]
pub struct FormattedObjectRelation {
    pub id: i32,
    /*    #[tabled(rename = "FromClass")]
    pub from_class: String,
    #[tabled(rename = "ToClass")]
    pub to_class: String, */
    #[tabled(rename = "FromObject")]
    pub from_object: String,
    #[tabled(rename = "ToObject")]
    pub to_object: String,
    #[tabled(display_with = "tabled_display", rename = "Created")]
    pub created_at: chrono::NaiveDateTime,
    #[tabled(display_with = "tabled_display", rename = "Updated")]
    pub updated_at: chrono::NaiveDateTime,
}

impl FormattedClassRelation {
    pub fn new(class_relation: &ClassRelation, classmap: &HashMap<i32, Class>) -> Self {
        let from_class = if classmap.get(&class_relation.from_hubuum_class_id).is_some() {
            classmap
                .get(&class_relation.from_hubuum_class_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };

        let to_class = if classmap.get(&class_relation.to_hubuum_class_id).is_some() {
            classmap
                .get(&class_relation.to_hubuum_class_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };

        Self {
            id: class_relation.id,
            from_class,
            to_class,
            created_at: class_relation.created_at,
            updated_at: class_relation.updated_at,
        }
    }
}

impl OutputFormatterWithPadding for FormattedClassRelation {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        append_key_value("ClassFrom", &self.from_class, padding)?;
        append_key_value("ClassTo", &self.to_class, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(())
    }
}

impl FormattedObjectRelation {
    pub fn new(
        object_relation: &ObjectRelation,
        _class_relation: &ClassRelation,
        objectmap: &HashMap<i32, Object>,
        _classmap: &HashMap<i32, Class>,
    ) -> Self {
        /*
        let from_class = if classmap.get(&class_relation.from_hubuum_class_id).is_some() {
            classmap
                .get(&class_relation.from_hubuum_class_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };

        let to_class = if classmap.get(&class_relation.to_hubuum_class_id).is_some() {
            classmap
                .get(&class_relation.to_hubuum_class_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };
        */

        let from_object = if objectmap
            .get(&object_relation.from_hubuum_object_id)
            .is_some()
        {
            objectmap
                .get(&object_relation.from_hubuum_object_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };

        let to_object = if objectmap
            .get(&object_relation.to_hubuum_object_id)
            .is_some()
        {
            objectmap
                .get(&object_relation.to_hubuum_object_id)
                .unwrap()
                .name
                .clone()
        } else {
            "".to_string()
        };

        Self {
            id: object_relation.id,
            //            from_class,
            //            to_class,
            from_object,
            to_object,
            created_at: object_relation.created_at,
            updated_at: object_relation.updated_at,
        }
    }
}

impl OutputFormatterWithPadding for FormattedObjectRelation {
    fn format(&self, padding: usize) -> Result<(), AppError> {
        //        append_key_value("ClassFrom", &self.from_class, padding)?;
        //        append_key_value("ClassTo", &self.to_class, padding)?;
        append_key_value("ObjectFrom", &self.from_object, padding)?;
        append_key_value("ObjectTo", &self.to_object, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(())
    }
}
