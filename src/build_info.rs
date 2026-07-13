pub const VERSION: &str = env!("HUBUUM_CLI_VERSION");
pub const TARGET: &str = env!("HUBUUM_CLI_BUILD_TARGET");
pub const GIT_SHA: Option<&str> = option_env!("HUBUUM_CLI_BUILD_GIT_SHA");

pub fn git_sha() -> Option<&'static str> {
    GIT_SHA.filter(|sha| !sha.is_empty())
}
