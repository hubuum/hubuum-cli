use crate::domain::{
    RelatedClassTreeNode, RelatedObjectTreeNode, ResolvedClassRelationRecord,
    ResolvedObjectRelationRecord, ResolvedRelatedClassRecord, ResolvedRelatedObjectRecord,
};
use crate::errors::AppError;
use crate::output::{append_key_value, append_line};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for ResolvedClassRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassA", self.class_a.clone()),
            ("ClassB", self.class_b.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedClassRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "ClassA", "ClassB", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.class_a.clone(),
            self.class_b.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedObjectRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassA", self.class_a.clone()),
            ("ClassB", self.class_b.clone()),
            ("ObjectA", self.object_a.clone()),
            ("ObjectB", self.object_b.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id", "ClassA", "ClassB", "ObjectA", "ObjectB", "Created", "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.class_a.clone(),
            self.class_b.clone(),
            self.object_a.clone(),
            self.object_b.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedRelatedClassRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Depth", self.depth.to_string()),
            ("Path", self.path.join(" -> ")),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ResolvedRelatedClassRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Namespace",
            "Depth",
            "Path",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.namespace.clone(),
            self.depth.to_string(),
            self.path.join(" -> "),
            self.created_at.clone(),
            self.updated_at.clone(),
        ]
    }
}

impl DetailRenderable for ResolvedRelatedObjectRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Class", self.class.clone()),
            ("Depth", self.depth.to_string()),
            ("Path", self.path.join(" -> ")),
            ("Created", self.created_at.clone()),
            ("Updated", self.updated_at.clone()),
        ]
    }
}

impl TableRenderable for ResolvedRelatedObjectRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Class",
            "Namespace",
            "Depth",
            "Path",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.class.clone(),
            self.namespace.clone(),
            self.depth.to_string(),
            self.path.join(" -> "),
            self.created_at.clone(),
            self.updated_at.clone(),
        ]
    }
}

pub fn render_related_object_tree_with_key(
    key: &str,
    nodes: &[RelatedObjectTreeNode],
    padding: i8,
) -> Result<(), AppError> {
    render_keyed_relation_entries(key, &object_relation_entries(nodes), padding)
}

pub fn render_related_class_tree_with_key(
    key: &str,
    nodes: &[RelatedClassTreeNode],
    padding: i8,
) -> Result<(), AppError> {
    render_keyed_relation_entries(key, &class_relation_entries(nodes), padding)
}

fn render_keyed_relation_entries(
    key: &str,
    entries: &[String],
    padding: i8,
) -> Result<(), AppError> {
    let padding = padding as usize;
    if let Some((first, rest)) = entries.split_first() {
        append_key_value(key, first, padding)?;
        let continuation_prefix = " ".repeat(padding + 3);
        for entry in rest {
            append_line(format!("{continuation_prefix}{entry}"))?;
        }
    } else {
        append_key_value(key, "No relations.", padding)?;
    }

    Ok(())
}

fn object_relation_entries(nodes: &[RelatedObjectTreeNode]) -> Vec<String> {
    relation_entries(nodes, |node| node.label(), |node| &node.children)
}

fn class_relation_entries(nodes: &[RelatedClassTreeNode]) -> Vec<String> {
    relation_entries(nodes, |node| node.label(), |node| &node.children)
}

fn relation_entries<T, FLabel, FChildren>(
    nodes: &[T],
    label: FLabel,
    children: FChildren,
) -> Vec<String>
where
    FLabel: Copy + Fn(&T) -> String,
    FChildren: Copy + Fn(&T) -> &[T],
{
    let mut entries = Vec::new();
    for node in nodes {
        collect_relation_entries(node, Vec::new(), label, children, &mut entries);
    }
    entries
}

fn collect_relation_entries<T, FLabel, FChildren>(
    node: &T,
    mut path: Vec<String>,
    label: FLabel,
    children: FChildren,
    entries: &mut Vec<String>,
) where
    FLabel: Copy + Fn(&T) -> String,
    FChildren: Copy + Fn(&T) -> &[T],
{
    path.push(label(node));
    let node_children = children(node);
    if node_children.is_empty() {
        entries.push(path.join(" → "));
        return;
    }

    for child in node_children {
        collect_relation_entries(child, path.clone(), label, children, entries);
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;
    use crate::output::{reset_output, take_output};

    #[test]
    #[serial]
    fn render_related_object_tree_uses_hierarchy() {
        reset_output().expect("output should reset");
        render_related_object_tree_with_key(
            "Relations",
            &[RelatedObjectTreeNode {
                id: 2,
                class: "Jacks".to_string(),
                name: "Jack-1".to_string(),
                namespace: "default".to_string(),
                depth: 1,
                children: vec![RelatedObjectTreeNode {
                    id: 2,
                    class: "Rooms".to_string(),
                    name: "B701".to_string(),
                    namespace: "default".to_string(),
                    depth: 2,
                    children: vec![],
                }],
            }],
            14,
        )
        .expect("tree should render");

        let snapshot = take_output().expect("snapshot should exist");
        assert_eq!(
            snapshot.lines,
            vec!["Relations      : Jacks/Jack-1 → Rooms/B701"]
        );
    }

    #[test]
    fn render_related_object_tree_with_key_aligns_follow_on_entries() {
        reset_output().expect("output should reset");
        render_related_object_tree_with_key(
            "Relations",
            &[
                RelatedObjectTreeNode {
                    id: 1,
                    class: "Contacts".to_string(),
                    name: "Entry".to_string(),
                    namespace: "default".to_string(),
                    depth: 1,
                    children: vec![],
                },
                RelatedObjectTreeNode {
                    id: 2,
                    class: "Jacks".to_string(),
                    name: "Jack-1".to_string(),
                    namespace: "default".to_string(),
                    depth: 1,
                    children: vec![RelatedObjectTreeNode {
                        id: 3,
                        class: "Rooms".to_string(),
                        name: "B701".to_string(),
                        namespace: "default".to_string(),
                        depth: 2,
                        children: vec![],
                    }],
                },
            ],
            14,
        )
        .expect("tree should render");

        let snapshot = take_output().expect("snapshot should exist");
        assert_eq!(
            snapshot.lines,
            vec![
                "Relations      : Contacts/Entry",
                "                 Jacks/Jack-1 → Rooms/B701"
            ]
        );
    }
}
