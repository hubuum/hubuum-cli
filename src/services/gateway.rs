use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::NaiveDateTime;
use hubuum_client::{
    client::{sync::Resource, GetID},
    ApiResource, Authenticated, Class, ClassPost, ClassRelation, ClassRelationPost,
    FilterOperator, GroupPost, NamespacePost, Object, ObjectPatch, ObjectPost, ObjectRelation,
    ObjectRelationPost, SyncClient, UserPost,
};

use crate::domain::{
    ClassDetails, ClassRecord, CreatedUser, GroupDetails, GroupPermissionsRecord,
    GroupPermissionsSummary, GroupRecord, NamespacePermission, NamespacePermissionsView,
    NamespaceRecord, ObjectRecord, ResolvedClassRelationRecord, ResolvedObjectRecord,
    ResolvedObjectRelationRecord, UserRecord,
};
use crate::errors::AppError;

#[derive(Debug, Clone, Default)]
pub struct ClassFilter {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateClassInput {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub json_schema: Option<serde_json::Value>,
    pub validate_schema: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct CreateNamespaceInput {
    pub name: String,
    pub description: String,
    pub owner: String,
}

#[derive(Debug, Clone, Default)]
pub struct UserFilter {
    pub username: Option<String>,
    pub email: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone)]
pub struct CreateUserInput {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct CreateGroupInput {
    pub groupname: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct GroupFilter {
    pub name: Option<String>,
    pub name_startswith: Option<String>,
    pub name_endswith: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateObjectInput {
    pub name: String,
    pub class_name: String,
    pub namespace: String,
    pub description: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ObjectFilter {
    pub class_name: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectUpdateInput {
    pub name: String,
    pub class_name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub data: Option<serde_json::Value>,
}

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

#[derive(Clone)]
pub struct HubuumGateway {
    client: Arc<SyncClient<Authenticated>>,
}

impl HubuumGateway {
    pub fn new(client: Arc<SyncClient<Authenticated>>) -> Self {
        Self { client }
    }

    pub fn list_group_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .groups()
            .find()
            .execute()?
            .into_iter()
            .map(|group| group.groupname)
            .collect())
    }

    pub fn list_class_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .classes()
            .find()
            .execute()?
            .into_iter()
            .map(|class| class.name)
            .collect())
    }

    pub fn list_namespace_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .namespaces()
            .find()
            .execute()?
            .into_iter()
            .map(|namespace| namespace.name)
            .collect())
    }

    pub fn list_object_names_for_class(&self, class_name: &str) -> Result<Vec<String>, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        Ok(self
            .client
            .objects(class.id())
            .find()
            .execute()?
            .into_iter()
            .map(|object| object.name)
            .collect())
    }

    pub fn create_class(&self, input: CreateClassInput) -> Result<ClassRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().create(ClassPost {
            name: input.name,
            namespace_id: namespace.id(),
            description: input.description,
            json_schema: input.json_schema,
            validate_schema: input.validate_schema,
        })?;
        Ok(ClassRecord::from(class))
    }

    pub fn class_details(&self, name: &str) -> Result<ClassDetails, AppError> {
        let class = self.client.classes().select_by_name(name)?;
        let objects = class
            .objects()?
            .into_iter()
            .map(|object| ObjectRecord::from(object.resource()))
            .collect();

        Ok(ClassDetails {
            class: ClassRecord::from(class.resource()),
            objects,
        })
    }

    pub fn delete_class(&self, name: &str) -> Result<(), AppError> {
        self.client.classes().select_by_name(name)?.delete()?;
        Ok(())
    }

    pub fn list_classes(&self, filter: ClassFilter) -> Result<Vec<ClassRecord>, AppError> {
        let mut search = self.client.classes().find();
        if let Some(name) = filter.name {
            search = search.add_filter(
                "name",
                FilterOperator::IContains { is_negated: false },
                name,
            );
        }
        if let Some(description) = filter.description {
            search = search.add_filter(
                "description",
                FilterOperator::IContains { is_negated: false },
                description,
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(ClassRecord::from)
            .collect())
    }

    pub fn create_namespace(&self, input: CreateNamespaceInput) -> Result<NamespaceRecord, AppError> {
        let group = self.client.groups().select_by_name(&input.owner)?;
        let namespace = self.client.namespaces().create(NamespacePost {
            name: input.name,
            description: input.description,
            group_id: group.id(),
        })?;
        Ok(NamespaceRecord::from(namespace))
    }

    pub fn list_namespaces(
        &self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<Vec<NamespaceRecord>, AppError> {
        let mut search = self.client.namespaces().find();
        if let Some(name) = name {
            search = search.add_filter(
                "name",
                FilterOperator::Contains { is_negated: false },
                name,
            );
        }
        if let Some(description) = description {
            search = search.add_filter(
                "description",
                FilterOperator::Contains { is_negated: false },
                description,
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(NamespaceRecord::from)
            .collect())
    }

    pub fn get_namespace(&self, name: &str) -> Result<NamespaceRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(name)?;
        Ok(NamespaceRecord::from(namespace.resource()))
    }

    pub fn delete_namespace(&self, name: &str) -> Result<(), AppError> {
        let namespace = self.client.namespaces().select_by_name(name)?;
        self.client.namespaces().delete(namespace.id())?;
        Ok(())
    }

    pub fn list_namespace_permissions(
        &self,
        name: &str,
    ) -> Result<NamespacePermissionsView, AppError> {
        let permissions = self.client.namespaces().select_by_name(name)?.permissions()?;
        let entries = permissions
            .iter()
            .cloned()
            .map(GroupPermissionsRecord::from)
            .collect::<Vec<_>>();
        let summary = permissions
            .into_iter()
            .map(GroupPermissionsSummary::from)
            .collect::<Vec<_>>();

        Ok(NamespacePermissionsView { entries, summary })
    }

    pub fn grant_namespace_permissions(
        &self,
        namespace_name: &str,
        group_name: &str,
        permissions: &[NamespacePermission],
    ) -> Result<(), AppError> {
        let namespace = self.client.namespaces().select_by_name(namespace_name)?;
        let group = self.client.groups().select_by_name(group_name)?;
        namespace.grant_permissions(
            group.id(),
            permissions.iter().map(|permission| permission.api_name()).collect(),
        )?;
        Ok(())
    }

    pub fn create_user(&self, input: CreateUserInput) -> Result<CreatedUser, AppError> {
        let user = self.client.users().create(UserPost {
            username: input.username,
            email: input.email,
            password: input.password.clone(),
        })?;

        Ok(CreatedUser {
            user: UserRecord::from(user),
            password: input.password,
        })
    }

    pub fn find_user(&self, filter: UserFilter) -> Result<UserRecord, AppError> {
        let mut search = self.client.users().find();
        if let Some(username) = filter.username {
            search = search.add_filter(
                "username",
                FilterOperator::IContains { is_negated: false },
                username,
            );
        }
        if let Some(email) = filter.email {
            search = search.add_filter(
                "email",
                FilterOperator::IContains { is_negated: false },
                email,
            );
        }
        if let Some(created_at) = filter.created_at {
            search = search.add_filter(
                "created_at",
                FilterOperator::Equals { is_negated: false },
                created_at.to_string(),
            );
        }
        if let Some(updated_at) = filter.updated_at {
            search = search.add_filter(
                "updated_at",
                FilterOperator::Equals { is_negated: false },
                updated_at.to_string(),
            );
        }
        let user = search.execute_expecting_single_result()?;
        Ok(UserRecord::from(user))
    }

    pub fn list_users(&self, filter: UserFilter) -> Result<Vec<UserRecord>, AppError> {
        let mut search = self.client.users().find();
        if let Some(username) = filter.username {
            search = search.add_filter(
                "username",
                FilterOperator::IContains { is_negated: false },
                username,
            );
        }
        if let Some(email) = filter.email {
            search = search.add_filter(
                "email",
                FilterOperator::IContains { is_negated: false },
                email,
            );
        }
        if let Some(created_at) = filter.created_at {
            search = search.add_filter(
                "created_at",
                FilterOperator::Equals { is_negated: false },
                created_at.to_string(),
            );
        }
        if let Some(updated_at) = filter.updated_at {
            search = search.add_filter(
                "updated_at",
                FilterOperator::Equals { is_negated: false },
                updated_at.to_string(),
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(UserRecord::from)
            .collect())
    }

    pub fn delete_user(&self, username: &str) -> Result<(), AppError> {
        let user = self.client.users().select_by_name(username)?;
        self.client.users().delete(user.id())?;
        Ok(())
    }

    pub fn create_group(&self, input: CreateGroupInput) -> Result<GroupRecord, AppError> {
        let group = self.client.groups().create(GroupPost {
            groupname: input.groupname,
            description: input.description,
        })?;
        Ok(GroupRecord::from(group))
    }

    pub fn add_user_to_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let user = self.client.users().select_by_name(username)?;
        group.add_user(user.id())?;
        Ok(())
    }

    pub fn remove_user_from_group(&self, group_name: &str, username: &str) -> Result<(), AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let user = self.client.users().select_by_name(username)?;
        group.remove_user(user.id())?;
        Ok(())
    }

    pub fn group_details(&self, group_name: &str) -> Result<GroupDetails, AppError> {
        let group = self.client.groups().select_by_name(group_name)?;
        let members = group
            .members()?
            .into_iter()
            .map(|user| UserRecord::from(user.resource()))
            .collect::<Vec<_>>();

        Ok(GroupDetails {
            group: GroupRecord::from(group.resource()),
            members,
        })
    }

    pub fn list_groups(&self, filter: GroupFilter) -> Result<Vec<GroupRecord>, AppError> {
        let mut search = self.client.groups().find();

        if let Some(name) = filter.name {
            search = search.add_filter(
                "groupname",
                FilterOperator::IContains { is_negated: false },
                name,
            );
        }
        if let Some(name_startswith) = filter.name_startswith {
            search = search.add_filter(
                "groupname",
                FilterOperator::StartsWith { is_negated: false },
                name_startswith,
            );
        }
        if let Some(name_endswith) = filter.name_endswith {
            search = search.add_filter(
                "groupname",
                FilterOperator::EndsWith { is_negated: false },
                name_endswith,
            );
        }
        if let Some(description) = filter.description {
            search = search.add_filter(
                "description",
                FilterOperator::IContains { is_negated: false },
                description,
            );
        }

        Ok(search
            .execute()?
            .into_iter()
            .map(GroupRecord::from)
            .collect())
    }

    pub fn create_object(&self, input: CreateObjectInput) -> Result<ResolvedObjectRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let class = self.client.classes().select_by_name(&input.class_name)?;

        let object = self.client.objects(class.id()).create(ObjectPost {
            name: input.name,
            hubuum_class_id: class.id(),
            namespace_id: namespace.id(),
            description: input.description,
            data: input.data,
        })?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let mut namespacemap = HashMap::new();
        namespacemap.insert(namespace.id(), namespace.resource().clone());

        Ok(ResolvedObjectRecord::new(&object, &classmap, &namespacemap))
    }

    pub fn object_details(
        &self,
        class_name: &str,
        object_name: &str,
    ) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        let namespace = self.client.namespaces().select(object.resource().namespace_id)?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let mut namespacemap = HashMap::new();
        namespacemap.insert(namespace.id(), namespace.resource().clone());

        Ok(ResolvedObjectRecord::new(
            object.resource(),
            &classmap,
            &namespacemap,
        ))
    }

    pub fn delete_object(&self, class_name: &str, object_name: &str) -> Result<(), AppError> {
        let class = self.client.classes().select_by_name(class_name)?;
        let object = class.object_by_name(object_name)?;
        self.client.objects(class.id()).delete(object.id())?;
        Ok(())
    }

    pub fn list_objects(&self, filter: ObjectFilter) -> Result<Vec<ResolvedObjectRecord>, AppError> {
        let class = self.client.classes().select_by_name(&filter.class_name)?;
        let mut search = self.client.objects(class.id()).find();

        if let Some(name) = filter.name {
            search = search.add_filter(
                "name",
                FilterOperator::IContains { is_negated: false },
                name,
            );
        }
        if let Some(description) = filter.description {
            search = search.add_filter(
                "description",
                FilterOperator::IContains { is_negated: false },
                description,
            );
        }

        let objects = search.execute()?;
        if objects.is_empty() {
            return Ok(Vec::new());
        }

        let classmap = find_entities_by_ids(&self.client.classes(), &objects, |object| object.hubuum_class_id)?;
        let namespacemap =
            find_entities_by_ids(&self.client.namespaces(), &objects, |object| object.namespace_id)?;

        Ok(objects
            .iter()
            .map(|object| ResolvedObjectRecord::new(object, &classmap, &namespacemap))
            .collect())
    }

    pub fn update_object(&self, input: ObjectUpdateInput) -> Result<ResolvedObjectRecord, AppError> {
        let class = self.client.classes().select_by_name(&input.class_name)?;
        let object = class.object_by_name(&input.name)?;

        let mut patch = ObjectPatch::default();
        patch.data = input.data;

        if let Some(namespace) = input.namespace {
            let namespace = self.client.namespaces().select_by_name(&namespace)?;
            patch.namespace_id = Some(namespace.id());
        }
        if let Some(rename) = input.rename {
            patch.name = Some(rename);
        }
        if let Some(description) = input.description {
            patch.description = Some(description);
        }

        let result = self.client.objects(class.id()).update(object.id(), patch)?;
        let namespace = self.client.namespaces().select(result.namespace_id)?;

        let mut classmap = HashMap::new();
        classmap.insert(class.id(), class.resource().clone());

        let mut namespacemap = HashMap::new();
        namespacemap.insert(namespace.id(), namespace.resource().clone());

        Ok(ResolvedObjectRecord::new(&result, &classmap, &namespacemap))
    }

    pub fn create_class_relation(
        &self,
        class_from: &str,
        class_to: &str,
    ) -> Result<ResolvedClassRelationRecord, AppError> {
        let (class_from, class_to) = self.class_pair(class_from, class_to)?;
        let relation = self.client.class_relation().create(ClassRelationPost {
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

        let relation = self.client.object_relation().create(ObjectRelationPost {
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
        let object_relation = self.find_object_relation(&class_relation, &object_from, &object_to)?;

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

        if let (Some(class_from_name), Some(class_to_name)) = (&filter.class_from, &filter.class_to) {
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
            let target = if swapped { "from_objects" } else { "to_objects" };
            query = query.add_filter_equals(target, object_from.id);
        }

        if let Some(object_to_name) = &filter.object_to {
            let object_to = self.find_object_by_name(class_to.id, object_to_name)?;
            let target = if swapped { "to_objects" } else { "from_objects" };
            query = query.add_filter_equals(target, object_to.id);
        }

        let object_relations = query.execute()?;
        if object_relations.is_empty() {
            return Ok(Vec::new());
        }

        let object_map = self.object_map_for_relation(&object_relations, class_from.id, class_to.id)?;
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

    fn class_pair(&self, class_from: &str, class_to: &str) -> Result<(Class, Class), AppError> {
        Ok((
            self.client.classes().select_by_name(class_from)?.resource().clone(),
            self.client.classes().select_by_name(class_to)?.resource().clone(),
        ))
    }

    fn class_map_from_classes<'a, I>(&self, classes: I) -> HashMap<i32, Class>
    where
        I: IntoIterator<Item = &'a Class>,
    {
        classes.into_iter().map(|class| (class.id, class.clone())).collect()
    }

    fn class_map_from_relation_ids(
        &self,
        relations: &[ClassRelation],
    ) -> Result<HashMap<i32, Class>, AppError> {
        let mut class_ids = HashSet::new();
        for relation in relations {
            class_ids.insert(relation.from_hubuum_class_id);
            class_ids.insert(relation.to_hubuum_class_id);
        }

        let joined = class_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        Ok(self
            .client
            .classes()
            .find()
            .add_filter_id(joined)
            .execute()?
            .into_iter()
            .map(|class| (class.id, class))
            .collect())
    }

    fn object_map_for_relation(
        &self,
        relations: &[ObjectRelation],
        from_class_id: i32,
        to_class_id: i32,
    ) -> Result<HashMap<i32, Object>, AppError> {
        let joined = relations
            .iter()
            .flat_map(|relation| {
                [
                    relation.from_hubuum_object_id.to_string(),
                    relation.to_hubuum_object_id.to_string(),
                ]
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut objects = HashMap::new();

        for object in self
            .client
            .objects(from_class_id)
            .find()
            .add_filter_equals("id", &joined)
            .execute()?
        {
            objects.insert(object.id, object);
        }

        for object in self
            .client
            .objects(to_class_id)
            .find()
            .add_filter_equals("id", &joined)
            .execute()?
        {
            objects.insert(object.id, object);
        }

        Ok(objects)
    }

    fn find_class_relation(&self, class_from_id: i32, class_to_id: i32) -> Result<ClassRelation, AppError> {
        Ok(self
            .client
            .class_relation()
            .find()
            .add_filter_equals("from_classes", class_from_id)
            .add_filter_equals("to_classes", class_to_id)
            .execute_expecting_single_result()?)
    }

    fn find_object_by_name(&self, class_id: i32, name: &str) -> Result<Object, AppError> {
        Ok(self
            .client
            .objects(class_id)
            .find()
            .add_filter_name_exact(name)
            .execute_expecting_single_result()?)
    }

    fn find_object_relation(
        &self,
        class_relation: &ClassRelation,
        object_from: &Object,
        object_to: &Object,
    ) -> Result<ObjectRelation, AppError> {
        Ok(self
            .client
            .object_relation()
            .find()
            .add_filter_equals("id", class_relation.id)
            .add_filter_equals("to_objects", object_to.id)
            .add_filter_equals("from_objects", object_from.id)
            .execute_expecting_single_result()?)
    }
}

fn find_entities_by_ids<T, I, F>(
    resource: &Resource<T>,
    objects: I,
    extract_id: F,
) -> Result<HashMap<i32, T::GetOutput>, AppError>
where
    T: ApiResource,
    I: IntoIterator,
    I::Item: Copy,
    F: Fn(I::Item) -> i32,
    T::GetOutput: GetID,
{
    let ids = objects
        .into_iter()
        .map(extract_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let results = resource
        .find()
        .add_filter("id", FilterOperator::Equals { is_negated: false }, ids)
        .execute()?;

    Ok(results
        .into_iter()
        .map(|entity| (entity.id(), entity))
        .collect())
}

fn validate_object_names(target: &RelationTarget) -> Result<(&str, &str), AppError> {
    match (target.object_from.as_deref(), target.object_to.as_deref()) {
        (Some(from), Some(to)) => Ok((from, to)),
        (None, _) => Err(AppError::MissingOptions(vec!["object_from".to_string()])),
        (_, None) => Err(AppError::MissingOptions(vec!["object_to".to_string()])),
    }
}
