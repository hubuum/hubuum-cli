use cli_command_derive::CommandArgs;
use hubuum_client::{CredentialActivityPageOptions, TokenId};

use super::builder::{catalog_command, CommandDocs};
use super::{required_option_or_pos, CliCommand};
use crate::autocomplete::{service_account_token_ids, service_accounts, user_token_ids, users};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::command_line::rebuild_with_replaced_options;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::append_json;
use crate::list_query::PARTIAL_PIPELINE_WARNING;
use crate::output::{
    add_warning, append_line, append_pipeline_suffix, has_pipeline, set_next_page_command,
    RenderFormat,
};
use crate::services::{AppServices, CredentialActivityInput, CredentialOwner};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    let docs = CommandDocs {
        about: Some("Show token issuance provenance and recent credential sources"),
        long_about: Some("Requires token-management authority for the owner. Rotating access tokens share session-family evidence. Activity is best-effort, includes authentication preceding authorization denial, and is not a complete request ledger. Results preserve collection status, retention, source cap, and overflow evidence. Pagination uses --offset (0–256) and --limit (1–100), not cursors."),
        examples: Some("--token-id 42 --limit 25\n--token-id 42 --all --output json"),
    };
    builder
        .add_command(
            &["me", "token"],
            catalog_command("activity", MyTokenActivity::default(), docs),
        )
        .add_command(
            &["user", "token"],
            catalog_command("activity", UserTokenActivity::default(), docs),
        )
        .add_command(
            &["service-account", "token"],
            catalog_command("activity", ServiceTokenActivity::default(), docs),
        );
}

macro_rules! activity_command {
    ($name:ident $(, $token_completion:literal)? { $($fields:tt)* } |$query:ident, $tokens:ident| $owner:expr, $position:expr) => {
        #[derive(Clone, Default, CommandArgs)]
        struct $name {
            #[option(long = "token-id", help = "Positive credential ID" $(, autocomplete = $token_completion)?)]
            token_id: Option<TokenId>,
            #[option(long = "offset", help = "Source offset, 0–256 (default 0)")]
            offset: Option<usize>,
            #[option(long = "limit", help = "Sources per page, 1–100 (default 50)")]
            limit: Option<usize>,
            #[option(long = "all", help = "Fetch all remaining source pages", flag = true)]
            all: bool,
            $($fields)*
        }
        impl CliCommand for $name {
            const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
            const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
            fn execute(&self, services: &AppServices, $tokens: &CommandTokenizer) -> Result<(), AppError> {
                let $query = Self::parse_tokens($tokens)?;
                let page = CredentialActivityPageOptions::default().offset($query.offset.unwrap_or(0))?.limit($query.limit.unwrap_or(50))?;
                let token_id = required_option_or_pos($query.token_id, $tokens, $position, "token-id")?;
                let input = CredentialActivityInput::new(token_id, page)?.all($query.all);
                let owner = $owner;
                let result = services.gateway().credential_activity(owner, input)?;
                if result.next_offset.is_some() && has_pipeline()? { add_warning(PARTIAL_PIPELINE_WARNING)?; }
                // Keep the complete evidence envelope in every output/pipeline.
                append_json(&result)?;
                if super::render_format($tokens)? == RenderFormat::Text {
                    if let Some(offset) = result.next_offset {
                        let next = rebuild_with_replaced_options($tokens, &["--offset"], [("--offset", Some(offset.to_string().as_str()))]);
                        set_next_page_command(append_pipeline_suffix(next)?)?;
                        append_line("More credential sources are available; use 'next' or --offset.")?;
                    }
                }
                Ok(())
            }
        }
    }
}

activity_command!(
    MyTokenActivity {} | query,
    tokens | CredentialOwner::Current,
    0
);
activity_command!(UserTokenActivity, "user_token_ids" {
    #[option(long = "username", help = "Token owner's username", autocomplete = "users")]
    username: Option<String>,
} |query, tokens| CredentialOwner::User(required_option_or_pos(query.username, tokens, 0, "username")?), 1);
activity_command!(ServiceTokenActivity, "service_account_token_ids" {
    #[option(long = "name", help = "Token owner's service-account name", autocomplete = "service_accounts")]
    name: Option<String>,
} |query, tokens| CredentialOwner::ServiceAccount(required_option_or_pos(query.name, tokens, 0, "name")?), 1);
