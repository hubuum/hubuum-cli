use std::time::Duration;

use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, CliCommand};
use crate::build_info;
use crate::catalog::CommandCatalogBuilder;
use crate::config::get_config;
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

const SERVER_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &[],
        catalog_command(
            "version",
            Version::default(),
            CommandDocs {
                about: Some("Show CLI build information"),
                long_about: Some(
                    "Show the CLI version, build target, and commit identity. Use --server to also query the configured Hubuum server's OpenAPI version.",
                ),
                examples: Some("  version\n  version --server\n  version --output json"),
            },
        ),
    );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct Version {
    #[option(
        long = "server",
        help = "Also query the configured Hubuum server version",
        flag = "true"
    )]
    pub server: Option<bool>,
}

impl CliCommand for Version {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_version(tokens)
    }
}

#[derive(Debug, Serialize)]
struct VersionInfo {
    cli_version: &'static str,
    git_commit: Option<&'static str>,
    target: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenApiDocument {
    info: OpenApiInfo,
}

#[derive(Debug, Deserialize)]
struct OpenApiInfo {
    version: String,
}

pub(crate) fn render_version(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = Version::parse_tokens(tokens)?;
    let server_version = query
        .server
        .unwrap_or(false)
        .then(fetch_server_version)
        .transpose()?;
    let info = VersionInfo {
        cli_version: build_info::VERSION,
        git_commit: build_info::git_sha(),
        target: build_info::TARGET,
        server_version,
    };

    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(&info)?)?,
        OutputFormat::Text => {
            append_key_value("CLI", info.cli_version, 10)?;
            if let Some(git_commit) = info.git_commit {
                append_key_value("Commit", git_commit, 10)?;
            }
            append_key_value("Target", info.target, 10)?;
            if let Some(server_version) = info.server_version {
                append_key_value("Server", server_version, 10)?;
            }
        }
    }

    Ok(())
}

fn fetch_server_version() -> Result<String, AppError> {
    let config = get_config();
    let url = format!(
        "{}://{}:{}/api-doc/openapi.json",
        config.server.protocol, config.server.hostname, config.server.port
    );
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(!config.server.ssl_validation)
        .timeout(SERVER_VERSION_TIMEOUT)
        .user_agent(format!("hubuum-cli/{}", build_info::VERSION))
        .build()
        .map_err(|error| server_version_error(&url, error))?;
    let response = client
        .get(&url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| server_version_error(&url, error))?;
    let document = response
        .json::<OpenApiDocument>()
        .map_err(|error| server_version_error(&url, error))?;

    Ok(normalize_version(&document.info.version))
}

fn server_version_error(url: &str, error: reqwest::Error) -> AppError {
    AppError::HttpError(format!("Unable to read server version from {url}: {error}"))
}

fn normalize_version(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_version;

    #[test]
    fn version_prefix_is_normalized() {
        assert_eq!(normalize_version("0.0.1"), "v0.0.1");
        assert_eq!(normalize_version("v0.0.1"), "v0.0.1");
    }
}
