use std::collections::HashMap;

use hubuum_client::{
    Class, ClassRelation, ClassWithPath, Namespace, Object, ObjectRelation, ObjectWithPath,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedClassRelationRecord {
    pub id: i32,
    pub class_a: String,
    pub class_b: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedClassRelationRecord {
    pub fn new(class_relation: &ClassRelation, classmap: &HashMap<i32, Class>) -> Self {
        let class_a = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let class_b = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();

        Self {
            id: class_relation.id,
            class_a,
            class_b,
            created_at: class_relation.created_at.to_string(),
            updated_at: class_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedObjectRelationRecord {
    pub id: i32,
    pub class_a: String,
    pub class_b: String,
    pub object_a: String,
    pub object_b: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedObjectRelationRecord {
    pub fn new(
        object_relation: &ObjectRelation,
        class_relation: &ClassRelation,
        objectmap: &HashMap<i32, Object>,
        classmap: &HashMap<i32, Class>,
    ) -> Self {
        let class_a = classmap
            .get(&class_relation.from_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let class_b = classmap
            .get(&class_relation.to_hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_default();
        let object_a = objectmap
            .get(&object_relation.from_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        let object_b = objectmap
            .get(&object_relation.to_hubuum_object_id)
            .map(|object| object.name.clone())
            .unwrap_or_default();

        Self {
            id: object_relation.id,
            class_a,
            class_b,
            object_a,
            object_b,
            created_at: object_relation.created_at.to_string(),
            updated_at: object_relation.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedClassRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub depth: usize,
    pub path: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedRelatedClassRecord {
    pub fn new(
        class: &ClassWithPath,
        namespacemap: &HashMap<i32, Namespace>,
        path_labels: Vec<String>,
    ) -> Self {
        let namespace = namespacemap
            .get(&class.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| class.namespace_id.to_string());

        Self {
            id: class.id,
            name: class.name.clone(),
            description: class.description.clone(),
            namespace,
            depth: class.path.len().saturating_sub(1),
            path: path_labels,
            created_at: class.created_at.to_string(),
            updated_at: class.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedObjectRecord {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub namespace: String,
    pub class: String,
    pub depth: usize,
    pub path: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ResolvedRelatedObjectRecord {
    pub fn new(
        object: &ObjectWithPath,
        classmap: &HashMap<i32, Class>,
        namespacemap: &HashMap<i32, Namespace>,
        path_labels: Vec<String>,
    ) -> Self {
        let namespace = namespacemap
            .get(&object.namespace_id)
            .map(|namespace| namespace.name.clone())
            .unwrap_or_else(|| object.namespace_id.to_string());
        let class = classmap
            .get(&object.hubuum_class_id)
            .map(|class| class.name.clone())
            .unwrap_or_else(|| object.hubuum_class_id.to_string());

        Self {
            id: object.id,
            name: object.name.clone(),
            description: object.description.clone(),
            namespace,
            class,
            depth: object.path.len().saturating_sub(1),
            path: path_labels,
            created_at: object.created_at.to_string(),
            updated_at: object.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedObjectGraph {
    pub objects: Vec<ResolvedRelatedObjectRecord>,
    pub relations: Vec<ResolvedObjectRelationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelatedClassGraph {
    pub classes: Vec<ResolvedRelatedClassRecord>,
    pub relations: Vec<ResolvedClassRelationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelatedObjectTreeNode {
    pub id: i32,
    pub class: String,
    pub name: String,
    pub namespace: String,
    pub depth: usize,
    pub children: Vec<RelatedObjectTreeNode>,
}

impl RelatedObjectTreeNode {
    pub fn label(&self) -> String {
        format!("{}/{}", self.class, self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelatedClassTreeNode {
    pub id: i32,
    pub name: String,
    pub namespace: String,
    pub depth: usize,
    pub children: Vec<RelatedClassTreeNode>,
}

impl RelatedClassTreeNode {
    pub fn label(&self) -> String {
        self.name.clone()
    }
}

pub fn build_related_object_tree(
    objects: &[ObjectWithPath],
    class_map: &HashMap<i32, Class>,
    namespace_map: &HashMap<i32, Namespace>,
    root_object_id: i32,
    root_class_id: i32,
    ignore_same_class: bool,
) -> Vec<RelatedObjectTreeNode> {
    let object_class_map = objects
        .iter()
        .map(|object| (object.id, object.hubuum_class_id))
        .chain(std::iter::once((root_object_id, root_class_id)))
        .collect::<HashMap<_, _>>();
    let nodes = objects
        .iter()
        .map(|object| {
            (
                object.id,
                RelatedObjectTreeNode {
                    id: object.id,
                    class: class_map
                        .get(&object.hubuum_class_id)
                        .map(|class| class.name.clone())
                        .unwrap_or_else(|| object.hubuum_class_id.to_string()),
                    name: object.name.clone(),
                    namespace: namespace_map
                        .get(&object.namespace_id)
                        .map(|namespace| namespace.name.clone())
                        .unwrap_or_else(|| object.namespace_id.to_string()),
                    depth: object.path.len().saturating_sub(1),
                    children: Vec::new(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let mut roots = Vec::new();
    for object in objects {
        if ignore_same_class && object.hubuum_class_id == root_class_id {
            continue;
        }

        let mut path = object
            .path
            .iter()
            .copied()
            .filter(|object_id| *object_id != root_object_id)
            .filter(|object_id| {
                !ignore_same_class
                    || object_class_map.get(object_id).copied().unwrap_or_default() != root_class_id
            })
            .collect::<Vec<_>>();
        if path.last().copied() != Some(object.id) {
            path.push(object.id);
        }
        insert_object_path(&mut roots, &path, &nodes);
    }
    sort_object_tree(&mut roots);
    roots
}

pub fn build_related_class_tree(
    classes: &[ClassWithPath],
    namespace_map: &HashMap<i32, Namespace>,
    root_class_id: i32,
    ignore_same_class: bool,
) -> Vec<RelatedClassTreeNode> {
    let nodes = classes
        .iter()
        .map(|class| {
            (
                class.id,
                RelatedClassTreeNode {
                    id: class.id,
                    name: class.name.clone(),
                    namespace: namespace_map
                        .get(&class.namespace_id)
                        .map(|namespace| namespace.name.clone())
                        .unwrap_or_else(|| class.namespace_id.to_string()),
                    depth: class.path.len().saturating_sub(1),
                    children: Vec::new(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let mut roots = Vec::new();
    for class in classes {
        let mut path = class
            .path
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, class_id)| !(*index == 0 && *class_id == root_class_id))
            .filter(|(_, class_id)| !ignore_same_class || *class_id != root_class_id)
            .map(|(_, class_id)| class_id)
            .collect::<Vec<_>>();
        if (!ignore_same_class || class.id != root_class_id)
            && path.last().copied() != Some(class.id)
        {
            path.push(class.id);
        }
        insert_class_path(&mut roots, &path, &nodes);
    }
    sort_class_tree(&mut roots);
    roots
}

fn insert_object_path(
    tree: &mut Vec<RelatedObjectTreeNode>,
    path: &[i32],
    nodes: &HashMap<i32, RelatedObjectTreeNode>,
) {
    let Some((&node_id, rest)) = path.split_first() else {
        return;
    };
    let Some(existing) = tree.iter_mut().find(|node| node.id == node_id) else {
        if let Some(node) = nodes.get(&node_id) {
            tree.push(node.clone());
            let len = tree.len();
            insert_object_path(&mut tree[len - 1].children, rest, nodes);
        }
        return;
    };
    insert_object_path(&mut existing.children, rest, nodes);
}

fn insert_class_path(
    tree: &mut Vec<RelatedClassTreeNode>,
    path: &[i32],
    nodes: &HashMap<i32, RelatedClassTreeNode>,
) {
    let Some((&node_id, rest)) = path.split_first() else {
        return;
    };
    let Some(existing) = tree.iter_mut().find(|node| node.id == node_id) else {
        if let Some(node) = nodes.get(&node_id) {
            tree.push(node.clone());
            let len = tree.len();
            insert_class_path(&mut tree[len - 1].children, rest, nodes);
        }
        return;
    };
    insert_class_path(&mut existing.children, rest, nodes);
}

fn sort_object_tree(nodes: &mut [RelatedObjectTreeNode]) {
    nodes.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    for node in nodes {
        sort_object_tree(&mut node.children);
    }
}

fn sort_class_tree(nodes: &mut [RelatedClassTreeNode]) {
    nodes.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    for node in nodes {
        sort_class_tree(&mut node.children);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namespace(id: i32, name: &str) -> Namespace {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "description": "",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }))
        .expect("namespace fixture should deserialize")
    }

    fn class(id: i32, namespace_id: i32, name: &str) -> Class {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "description": "",
            "namespace": {
                "id": namespace_id,
                "name": "default",
                "description": "",
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-01-01T00:00:00Z"
            },
            "json_schema": {},
            "validate_schema": false,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }))
        .expect("class fixture should deserialize")
    }

    fn related_object(
        id: i32,
        class_id: i32,
        namespace_id: i32,
        name: &str,
        path: &[i32],
    ) -> ObjectWithPath {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "description": "",
            "namespace_id": namespace_id,
            "hubuum_class_id": class_id,
            "data": {},
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z",
            "path": path
        }))
        .expect("related object fixture should deserialize")
    }

    fn related_class(id: i32, namespace_id: i32, name: &str, path: &[i32]) -> ClassWithPath {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "description": "",
            "namespace_id": namespace_id,
            "json_schema": {},
            "validate_schema": false,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z",
            "path": path
        }))
        .expect("related class fixture should deserialize")
    }

    #[test]
    fn build_related_object_tree_folds_shared_paths() {
        let objects = vec![
            related_object(10, 2, 1, "Entry", &[9, 10]),
            related_object(11, 3, 1, "BL14=521.A7-UD7056", &[9, 11]),
            related_object(12, 4, 1, "B701", &[9, 11, 12]),
        ];
        let class_map = HashMap::from([
            (2, class(2, 1, "Contacts")),
            (3, class(3, 1, "Jacks")),
            (4, class(4, 1, "Rooms")),
        ]);
        let namespace_map = HashMap::from([(1, namespace(1, "default"))]);

        let tree = build_related_object_tree(&objects, &class_map, &namespace_map, 9, 1, false);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].label(), "Contacts/Entry");
        assert_eq!(tree[1].label(), "Jacks/BL14=521.A7-UD7056");
        assert_eq!(tree[1].children.len(), 1);
        assert_eq!(tree[1].children[0].label(), "Rooms/B701");
    }

    #[test]
    fn build_related_object_tree_ignores_same_class_but_keeps_other_branches() {
        let objects = vec![
            related_object(10, 1, 1, "SiblingHost", &[9, 10]),
            related_object(11, 3, 1, "Jack-1", &[9, 10, 11]),
            related_object(12, 4, 1, "Room-1", &[9, 10, 11, 12]),
        ];
        let class_map = HashMap::from([
            (1, class(1, 1, "Hosts")),
            (3, class(3, 1, "Jacks")),
            (4, class(4, 1, "Rooms")),
        ]);
        let namespace_map = HashMap::from([(1, namespace(1, "default"))]);

        let tree = build_related_object_tree(&objects, &class_map, &namespace_map, 9, 1, true);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].label(), "Jacks/Jack-1");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].label(), "Rooms/Room-1");
    }

    #[test]
    fn build_related_class_tree_filters_root_and_sorts() {
        let classes = vec![
            related_class(1, 1, "Root", &[1]),
            related_class(3, 1, "Rooms", &[1, 2, 3]),
            related_class(2, 1, "Jacks", &[1, 2]),
        ];
        let namespace_map = HashMap::from([(1, namespace(1, "default"))]);

        let tree = build_related_class_tree(&classes, &namespace_map, 1, true);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].label(), "Jacks");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].label(), "Rooms");
    }
}
