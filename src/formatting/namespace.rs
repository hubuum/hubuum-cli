use hubuum_client::{client::sync::Handle, GroupPermissionsResult, Namespace};
use serde::Serialize;
use tabled::Tabled;

use super::{append_key_value, OutputFormatter, OutputFormatterWithPadding};
use crate::errors::AppError;

impl OutputFormatterWithPadding for Namespace {
    fn format(&self, padding: usize) -> Result<Self, AppError> {
        append_key_value("Name", &self.name, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Created", self.created_at, padding)?;
        append_key_value("Updated", self.updated_at, padding)?;
        Ok(self.clone())
    }
}

impl OutputFormatterWithPadding for Handle<Namespace> {
    fn format(&self, padding: usize) -> Result<Self, AppError> {
        self.resource().format(padding)?;
        Ok(self.clone())
    }
}

impl OutputFormatterWithPadding for GroupPermissionsResult {
    fn format(&self, padding: usize) -> Result<Self, AppError> {
        append_key_value("Group", &self.group.groupname, padding)?;
        append_key_value("Namespace", &self.permission.namespace_id, padding)?;
        append_key_value("Permissions:", "", padding)?;

        let permission_groups: &[(&str, &[(&str, bool)])] = &[
            (
                "Namespace",
                &[
                    ("read", self.permission.has_read_namespace),
                    ("update", self.permission.has_update_namespace),
                    ("delete", self.permission.has_delete_namespace),
                    ("delegate", self.permission.has_delegate_namespace),
                ],
            ),
            (
                "Class",
                &[
                    ("create", self.permission.has_create_class),
                    ("read", self.permission.has_read_class),
                    ("update", self.permission.has_update_class),
                    ("delete", self.permission.has_delete_class),
                ],
            ),
            (
                "Object",
                &[
                    ("create", self.permission.has_create_object),
                    ("read", self.permission.has_read_object),
                    ("update", self.permission.has_update_object),
                    ("delete", self.permission.has_delete_object),
                ],
            ),
            (
                "Class Relation",
                &[
                    ("create", self.permission.has_create_class_relation),
                    ("read", self.permission.has_read_class_relation),
                    ("update", self.permission.has_update_class_relation),
                    ("delete", self.permission.has_delete_class_relation),
                ],
            ),
            (
                "Object Relation",
                &[
                    ("create", self.permission.has_create_object_relation),
                    ("read", self.permission.has_read_object_relation),
                    ("update", self.permission.has_update_object_relation),
                    ("delete", self.permission.has_delete_object_relation),
                ],
            ),
        ];

        for (group_name, permissions) in permission_groups {
            let enabled: Vec<&str> = permissions
                .iter()
                .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
                .collect();

            if !enabled.is_empty() {
                append_key_value(format!("  {group_name}"), &enabled.join(", "), padding)?;
            }
        }
        Ok(self.clone())
    }
}

#[derive(Debug, Clone, Tabled, Serialize)]
pub struct FormattedGroupPermissions {
    #[tabled(rename = "Group")]
    pub group: String,

    #[tabled(rename = "Namespace")]
    pub namespace: String,

    #[tabled(rename = "Class")]
    pub class: String,

    #[tabled(rename = "Object")]
    pub object: String,

    #[tabled(rename = "Class Relation")]
    pub class_relation: String,

    #[tabled(rename = "Object Relation")]
    pub object_relation: String,
}

impl From<GroupPermissionsResult> for FormattedGroupPermissions {
    fn from(gpr: GroupPermissionsResult) -> Self {
        fn fmt(perms: &[(&str, bool)]) -> String {
            perms
                .iter()
                .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
                .collect::<Vec<_>>()
                .join(", ")
        }

        let permissions = &gpr.permission;

        FormattedGroupPermissions {
            group: gpr.group.groupname,
            namespace: fmt(&[
                ("read", permissions.has_read_namespace),
                ("update", permissions.has_update_namespace),
                ("delete", permissions.has_delete_namespace),
                ("delegate", permissions.has_delegate_namespace),
            ]),
            class: fmt(&[
                ("create", permissions.has_create_class),
                ("read", permissions.has_read_class),
                ("update", permissions.has_update_class),
                ("delete", permissions.has_delete_class),
            ]),
            object: fmt(&[
                ("create", permissions.has_create_object),
                ("read", permissions.has_read_object),
                ("update", permissions.has_update_object),
                ("delete", permissions.has_delete_object),
            ]),
            class_relation: fmt(&[
                ("create", permissions.has_create_class_relation),
                ("read", permissions.has_read_class_relation),
                ("update", permissions.has_update_class_relation),
                ("delete", permissions.has_delete_class_relation),
            ]),
            object_relation: fmt(&[
                ("create", permissions.has_create_object_relation),
                ("read", permissions.has_read_object_relation),
                ("update", permissions.has_update_object_relation),
                ("delete", permissions.has_delete_object_relation),
            ]),
        }
    }
}

impl OutputFormatterWithPadding for Vec<GroupPermissionsResult> {
    fn format(&self, _padding: usize) -> Result<Self, AppError> {
        let formatted: Vec<FormattedGroupPermissions> =
            self.iter().cloned().map(Into::into).collect();
        formatted.format()?;
        Ok(self.clone())
    }
}
