use cli_command_derive::CliCommand;
use hubuum_client::{Authenticated, NamespacePost, SyncClient};
use serde::{Deserialize, Serialize};

use super::CliCommand;
use super::{CliCommandInfo, CliOption};

use crate::errors::AppError;
use crate::formatting::{OutputFormatter, OutputFormatterWithPadding};
use crate::tokenizer::CommandTokenizer;
use crate::traits::SingleItemOrWarning;

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct NamespaceNew {
    #[option(short = "n", long = "name", help = "Name of the namespace")]
    pub name: String,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the namespace"
    )]
    pub description: String,
    #[option(
        short = "o",
        long = "owner",
        help = "Name of the group owning namespace"
    )]
    pub owner: String,
}

impl NamespaceNew {
    fn into_post(&self, group_id: i32) -> NamespacePost {
        NamespacePost {
            name: self.name.clone(),
            description: self.description.clone(),
            group_id,
        }
    }
}

impl CliCommand for NamespaceNew {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;

        let group = client
            .groups()
            .find()
            .add_filter_name_exact(new.owner.clone())
            .execute()?
            .single_item_or_warning()?;

        let post = new.into_post(group.id);

        let namespace = client.namespaces().create(post)?;
        namespace.format(15)?;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CliCommand, Default)]
pub struct NamespaceList {
    #[option(short = "n", long = "name", help = "Name of the namespace")]
    pub name: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "Description of the namespace"
    )]
    pub description: Option<String>,
}

impl CliCommand for NamespaceList {
    fn execute(
        &self,
        client: &SyncClient<Authenticated>,
        tokens: &CommandTokenizer,
    ) -> Result<(), AppError> {
        let new = self.new_from_tokens(tokens)?;

        let search = client.namespaces().find();

        let search = match &new.name {
            Some(name) => search.add_filter(
                "name",
                hubuum_client::FilterOperator::Contains { is_negated: false },
                name.clone(),
            ),
            None => search,
        };

        let search = match &new.description {
            Some(description) => search.add_filter(
                "description",
                hubuum_client::FilterOperator::Contains { is_negated: false },
                description.clone(),
            ),
            None => search,
        };

        let namespaces = search.execute()?;
        namespaces.format()?;

        Ok(())
    }
}
