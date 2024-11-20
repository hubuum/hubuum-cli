use cli_command_derive::CliCommand;

use hubuum_client::{Authenticated, Class, ClassRelationPost, ObjectRelationPost, SyncClient};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::vec;

use super::{CliCommand, CliCommandInfo, CliOption};
use crate::autocomplete::{classes, objects_from_class_from, objects_from_class_to};
use crate::commands::shared::{
    find_class_by_name, find_class_relation, find_classes, find_object_by_name,
    find_object_relation, Commafy,
};
use crate::errors::AppError;
use crate::formatting::{
    FormattedClassRelation, FormattedObjectRelation, OutputFormatter, OutputFormatterWithPadding,
};
use crate::output::append_line;
use crate::tokenizer::CommandTokenizer;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Create a relationship",
    long_about = "Create a new relationship between classes or objects.",
    examples = r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#
)]
pub struct RelationNew {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Delete a relationship",
    long_about = "Delete a new relationship between classes or objects.",
    examples = r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#
)]
pub struct RelationDelete {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "List relationships",
    long_about = "List relationships between classes or objects.",
    examples = r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#
)]
pub struct RelationList {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: Option<String>,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: Option<String>,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
#[command_info(
    about = "Information about a relationships",
    long_about = "Show information about relationships between classes or objects.",
    examples = r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#
)]
pub struct RelationInfo {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

impl CliCommand for RelationNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        let (class_from, class_to) = find_classes(client, &new.class_from, &new.class_to)?;

        let mut class_map = HashMap::new();
        class_map.insert(class_from.id, class_from.clone());
        class_map.insert(class_to.id, class_to.clone());

        if new.object_from.is_none() && new.object_to.is_none() {
            create_class_relation(client, &class_from, &class_to, &class_map)?;
        } else {
            create_object_relation(client, new, &class_from, &class_to)?;
        }

        Ok(())
    }
}

impl CliCommand for RelationDelete {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        let (class_from, class_to) = find_classes(client, &new.class_from, &new.class_to)?;

        if new.object_from.is_none() && new.object_to.is_none() {
            delete_class_relation(client, &class_from, &class_to)?;
        } else {
            delete_object_relation(client, new, &class_from, &class_to)?;
        }

        Ok(())
    }
}

impl CliCommand for RelationList {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;

        let mut query = client.class_relation().find();

        let mut swapped = false;
        let mut from_class = None;
        let mut to_class = None;

        if new.class_from.is_some() && new.class_to.is_some() {
            from_class = Some(find_class_by_name(
                client,
                new.class_from.as_ref().unwrap(),
            )?);
            to_class = Some(find_class_by_name(client, new.class_to.as_ref().unwrap())?);

            let from = from_class.clone().unwrap();
            let to = to_class.clone().unwrap();

            if from.id > to.id {
                swapped = true;
                (from_class, to_class) = (to_class.clone(), from_class.clone())
            }

            query = query
                .add_filter_equals("from_classes", from_class.clone().unwrap().id)
                .add_filter_equals("to_classes", to_class.clone().unwrap().id);
        } else if new.class_from.is_some() {
            from_class = Some(find_class_by_name(
                client,
                new.class_from.as_ref().unwrap(),
            )?);
            query = query.add_filter_equals("from_classes", from_class.clone().unwrap().id);
        } else if new.class_to.is_some() {
            to_class = Some(find_class_by_name(client, new.class_to.as_ref().unwrap())?);
            query = query.add_filter_equals("to_classes", to_class.clone().unwrap().id);
        }

        let class_relations = query.execute()?;

        if class_relations.is_empty() {
            println!("No relations found");
            return Ok(());
        }

        let mut class_ids = vec![];
        for relation in &class_relations {
            class_ids.push(relation.from_hubuum_class_id);
            class_ids.push(relation.to_hubuum_class_id);
        }

        // Here we should filter out already known IDs...
        let class_ids_joined = class_ids.into_iter().commafy_unique();

        let classes = client
            .classes()
            .find()
            .add_filter_id(class_ids_joined)
            .execute()?;

        let mut class_map = HashMap::new();
        for class in &classes {
            class_map.insert(class.id, class.clone());
        }

        if class_relations.len() > 1 || (new.class_from.is_none() || new.class_to.is_none()) {
            let class_relations_formatted = class_relations
                .iter()
                .map(|r| FormattedClassRelation::new(r, &class_map))
                .collect::<Vec<_>>();
            class_relations_formatted.format()?;

            return Ok(());
        }

        if to_class.is_none() {
            if swapped {
                to_class = Some(
                    client
                        .classes()
                        .find()
                        .add_filter_id(class_relations[0].from_hubuum_class_id)
                        .execute_expecting_single_result()?,
                )
            } else {
                to_class = Some(
                    client
                        .classes()
                        .find()
                        .add_filter_id(class_relations[0].to_hubuum_class_id)
                        .execute_expecting_single_result()?,
                )
            }
        }

        if from_class.is_none() {
            if swapped {
                from_class = Some(
                    client
                        .classes()
                        .find()
                        .add_filter_id(class_relations[0].to_hubuum_class_id)
                        .execute_expecting_single_result()?,
                )
            } else {
                from_class = Some(
                    client
                        .classes()
                        .find()
                        .add_filter_id(class_relations[0].from_hubuum_class_id)
                        .execute_expecting_single_result()?,
                )
            }
        }

        let mut query = client
            .object_relation()
            .find()
            .add_filter_equals("class_relation", class_relations[0].id);

        if new.object_from.is_some() {
            let object_from = find_object_by_name(
                client,
                from_class.clone().unwrap().id,
                new.object_from.as_ref().unwrap(),
            )?;
            let target = if swapped {
                "from_objects"
            } else {
                "to_objects"
            };

            query = query.add_filter_equals(target, object_from.id);
        }

        if new.object_to.is_some() {
            let object_to = find_object_by_name(
                client,
                to_class.clone().unwrap().id,
                new.object_to.as_ref().unwrap(),
            )?;

            let target = if swapped {
                "to_objects"
            } else {
                "from_objects"
            };

            query = query.add_filter_equals(target, object_to.id);
        }

        let object_relations = query.execute()?;

        if object_relations.is_empty() {
            println!("No relations found");
            return Ok(());
        }

        // We don't know what classes each object is in, so we ask both classes for their objects
        // constrained to the IDs we have...
        let object_ids_joined = object_relations
            .iter()
            .flat_map(|r| {
                [
                    r.from_hubuum_object_id.to_string(),
                    r.to_hubuum_object_id.to_string(),
                ]
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut object_map = HashMap::new();

        let class_from_objects = client
            .objects(from_class.clone().unwrap().id)
            .find()
            .add_filter_equals("id", &object_ids_joined)
            .execute()?;
        for object in class_from_objects {
            object_map.insert(object.id, object.clone());
        }

        let class_to_objects = client
            .objects(to_class.clone().unwrap().id)
            .find()
            .add_filter_equals("id", &object_ids_joined)
            .execute()?;

        for object in class_to_objects {
            object_map.insert(object.id, object.clone());
        }

        let mut class_relation_corrected = class_relations[0].clone();
        if swapped {
            std::mem::swap(
                &mut class_relation_corrected.from_hubuum_class_id,
                &mut class_relation_corrected.to_hubuum_class_id,
            );
        }

        /*
        let formatted_class_relation =
            FormattedClassRelation::new(&class_relation_corrected, &class_map);
        formatted_class_relation.format(15)?;
        */

        let formatted_object_relations = object_relations
            .iter()
            .map(|r| {
                FormattedObjectRelation::new(r, &class_relation_corrected, &object_map, &class_map)
            })
            .collect::<Vec<_>>();

        formatted_object_relations.format()?;

        Ok(())
    }
}

impl CliCommand for RelationInfo {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = &self.new_from_tokens(tokens)?;
        if new.object_from.is_none() && new.object_to.is_none() {
            let (class_from, class_to) = find_classes(client, &new.class_from, &new.class_to)?;
            let mut classmap = HashMap::new();
            classmap.insert(class_from.id, class_from.clone());
            classmap.insert(class_to.id, class_to.clone());

            let rel = find_class_relation(client, class_from.id, class_to.id)?;
            let rel = FormattedClassRelation::new(&rel, &classmap);
            rel.format(15)?;
        } else {
            let (class_from, class_to) = find_classes(client, &new.class_from, &new.class_to)?;
            let object_from =
                find_object_by_name(client, class_from.id, new.object_from.as_ref().unwrap())?;
            let object_to =
                find_object_by_name(client, class_to.id, new.object_to.as_ref().unwrap())?;

            let mut objectmap = HashMap::new();
            objectmap.insert(object_from.id, object_from.clone());
            objectmap.insert(object_to.id, object_to.clone());

            let mut classmap = HashMap::new();
            classmap.insert(class_from.id, class_from.clone());
            classmap.insert(class_to.id, class_to.clone());

            let class_relation = find_class_relation(client, class_from.id, class_to.id)?;
            let object_relation =
                find_object_relation(client, &class_relation, &object_from, &object_to)?;
            let object_relation = FormattedObjectRelation::new(
                &object_relation,
                &class_relation,
                &objectmap,
                &classmap,
            );
            object_relation.format(15)?;
        }
        Ok(())
    }
}

fn create_class_relation(
    client: &SyncClient<Authenticated>,
    class_from: &Class,
    class_to: &Class,
    class_map: &HashMap<i32, Class>,
) -> Result<(), AppError> {
    let post = ClassRelationPost {
        from_hubuum_class_id: class_from.id,
        to_hubuum_class_id: class_to.id,
    };

    let relation = client.class_relation().create(post)?;
    let formatted_relation = FormattedClassRelation::new(&relation, class_map);
    formatted_relation.format(15)?;
    Ok(())
}

fn create_object_relation(
    client: &SyncClient<Authenticated>,
    new: &RelationNew,
    class_from: &Class,
    class_to: &Class,
) -> Result<(), AppError> {
    let (object_from, object_to) = validate_object_names(new)?;
    let class_relation = find_class_relation(client, class_from.id, class_to.id)?;
    let object_from = find_object_by_name(client, class_from.id, &object_from)?;
    let object_to = find_object_by_name(client, class_to.id, &object_to)?;

    let post = ObjectRelationPost {
        class_relation_id: class_relation.id,
        from_hubuum_object_id: object_from.id,
        to_hubuum_object_id: object_to.id,
    };

    let from_class = client
        .classes()
        .find()
        .add_filter_id(class_relation.from_hubuum_class_id)
        .execute_expecting_single_result()?;
    let to_class = client
        .classes()
        .find()
        .add_filter_id(class_relation.to_hubuum_class_id)
        .execute_expecting_single_result()?;

    let mut object_map = HashMap::new();
    let mut class_map = HashMap::new();
    let mut nsmap = HashMap::new();

    class_map.insert(from_class.id, from_class.clone());
    class_map.insert(to_class.id, to_class.clone());

    nsmap.insert(from_class.namespace.id, from_class.namespace.clone());
    nsmap.insert(to_class.namespace.id, to_class.namespace.clone());

    object_map.insert(object_from.id, object_from.clone());
    object_map.insert(object_to.id, object_to.clone());

    let relation = client.object_relation().create(post)?;
    let relation =
        FormattedObjectRelation::new(&relation, &class_relation, &object_map, &class_map);
    relation.format(15)?;
    Ok(())
}

fn delete_class_relation(
    client: &SyncClient<Authenticated>,
    class_from: &Class,
    class_to: &Class,
) -> Result<(), AppError> {
    let relation = find_class_relation(client, class_from.id, class_to.id)?;
    client.class_relation().delete(relation.id)?;
    append_line("Deleted class relation")?;
    Ok(())
}

fn delete_object_relation(
    client: &SyncClient<Authenticated>,
    new: &RelationDelete,
    class_from: &Class,
    class_to: &Class,
) -> Result<(), AppError> {
    let (object_from, object_to) = validate_object_names(new)?;
    let class_relation = find_class_relation(client, class_from.id, class_to.id)?;
    let object_from = find_object_by_name(client, class_from.id, &object_from)?;
    let object_to = find_object_by_name(client, class_to.id, &object_to)?;
    let relation = find_object_relation(client, &class_relation, &object_from, &object_to)?;
    client.object_relation().delete(relation.id)?;
    append_line(format!(
        "Deleted object relation ({} <> {}",
        object_from.name, object_to.name
    ))?;
    Ok(())
}

fn validate_object_names<T: HasObjectNames>(new: &T) -> Result<(String, String), AppError> {
    match (new.object_from(), new.object_to()) {
        (Some(from), Some(to)) => Ok((from.to_string(), to.to_string())),
        (None, _) => Err(AppError::MissingOptions(vec!["object_from".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object_to".to_string()])),
    }
}

trait HasObjectNames {
    fn object_from(&self) -> &Option<String>;
    fn object_to(&self) -> &Option<String>;
}

impl HasObjectNames for RelationNew {
    fn object_from(&self) -> &Option<String> {
        &self.object_from
    }
    fn object_to(&self) -> &Option<String> {
        &self.object_to
    }
}

impl HasObjectNames for RelationDelete {
    fn object_from(&self) -> &Option<String> {
        &self.object_from
    }
    fn object_to(&self) -> &Option<String> {
        &self.object_to
    }
}
