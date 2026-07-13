use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=HUBUUM_CLI_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=HUBUUM_CLI_BUILD_GIT_SHA");

    let package_version = env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let channel = env::var("HUBUUM_CLI_BUILD_CHANNEL").unwrap_or_default();
    let git_sha = env::var("HUBUUM_CLI_BUILD_GIT_SHA").unwrap_or_default();
    let short_sha = git_sha
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();

    let version = if channel == "main" && !short_sha.is_empty() {
        format!("v{package_version}+main.g{short_sha}")
    } else {
        format!("v{package_version}")
    };

    println!("cargo:rustc-env=HUBUUM_CLI_VERSION={version}");
    println!(
        "cargo:rustc-env=HUBUUM_CLI_BUILD_TARGET={}",
        env::var("TARGET").expect("Cargo build target")
    );
}
