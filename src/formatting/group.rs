use hubuum_client::{Group, HubuumDateTime};
use serde::Serialize;
use tabled::Tabled;

use super::{append_key_value, tabled_display, OutputFormatter};
use crate::config::get_config;
use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Tabled)]
pub struct FormattedGroup {
    #[tabled(rename = "Group")]
    pub groupname: String,
    #[tabled(rename = "Description")]
    pub description: String,
    #[tabled(display = "tabled_display", rename = "Created")]
    pub created_at: HubuumDateTime,
    #[tabled(display = "tabled_display", rename = "Updated")]
    pub updated_at: HubuumDateTime,
}

impl From<&Group> for FormattedGroup {
    fn from(group: &Group) -> Self {
        Self {
            groupname: group.groupname.clone(),
            description: group.description.clone(),
            created_at: group.created_at.clone(),
            updated_at: group.updated_at.clone(),
        }
    }
}

impl OutputFormatter for Group {
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        append_key_value("Name", &self.groupname, padding)?;
        append_key_value("Description", &self.description, padding)?;
        append_key_value("Created", &self.created_at, padding)?;
        append_key_value("Updated", &self.updated_at, padding)?;
        Ok(self.clone())
    }
}
