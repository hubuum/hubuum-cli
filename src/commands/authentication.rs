use cli_command_derive::CommandArgs;

use super::admin::render_structured_value;
use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::config::get_config;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::services::{AppServices, ProviderCandidate};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(&["admin","auth"],catalog_command("show",AuthenticationShow::default(),CommandDocs{
        about:Some("Inspect effective authentication providers, revisions, locks and replica application status"),
        long_about:Some("Reports credential presence only; stored credentials and aliases are never displayed. Local recovery remains available. Register each browser provider's callback path at the deployment's exact public browser origin."),
        ..Default::default()
    }));
    builder.add_command(&["admin","auth"],catalog_command("configure",AuthenticationConfigure::default(),CommandDocs{
        about:Some("Test complete desired provider configuration; activate only with explicit fresh approval"),
        long_about:Some("Read an owner-only JSON file or explicit stdin (-). The file contains group=authentication, expected_revision and the complete desired providers array. Each enabled LDAP provider needs verification credentials; OIDC/OAuth use actual browser sign-in. Tests do not provision accounts or issue sessions. --activate binds fresh approval to the exact verified candidate. Revision/lock/receipt conflicts are never retried. Use admin auth show afterward to inspect replica convergence; local recovery stays on."),
        ..Default::default()
    }));
}

#[derive(Clone, Default, CommandArgs)]
struct AuthenticationShow {}
impl CliCommand for AuthenticationShow {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        Self::parse_tokens(tokens)?;
        render_structured_value(
            services.gateway().authentication_settings()?,
            desired_format(tokens)?,
        )
    }
}

#[derive(Clone, Default, CommandArgs)]
struct AuthenticationConfigure {
    #[option(
        long = "file",
        help = "Owner-only complete configuration JSON file; - reads explicit stdin"
    )]
    file: String,
    #[option(
        long = "activate",
        help = "Fresh-approve and activate the exact verified candidate",
        flag = true
    )]
    activate: bool,
}
impl CliCommand for AuthenticationConfigure {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let candidate = ProviderCandidate::read(&query.file)?;
        let config = get_config();
        render_structured_value(
            services.gateway().configure_authentication(
                candidate,
                query.activate,
                config.server.no_browser,
            )?,
            desired_format(tokens)?,
        )
    }
}
