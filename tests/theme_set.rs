use std::fs::{read_to_string, write};
use std::path::Path;

use assert_cmd::{cargo::cargo_bin_cmd, Command};
use predicates::str::contains;
use serde_json::{from_slice, Value};
use tempfile::tempdir;

fn cli(config: &Path) -> Command {
    let mut command = cargo_bin_cmd!("hubuum-cli");
    command
        .env("XDG_CONFIG_HOME", config.parent().unwrap())
        .arg("--config")
        .arg(config)
        .args(["--hostname", "127.0.0.1", "--port", "1"]);
    command
}

fn json(config: &Path, args: &[&str]) -> Value {
    let output = cli(config)
        .args(args)
        .args(["--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!output.contains(&0x1b));
    from_slice(&output).unwrap()
}

#[test]
fn preview_is_ephemeral_and_shows_proposed_colors() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let initial = "# keep this comment\n[output]\ntheme = 'rose-pink'\n";
    write(&config, initial).unwrap();
    let before = json(&config, &["theme", "preview"]);
    let candidate = json(
        &config,
        &[
            "theme",
            "set",
            "url",
            "--fg",
            "teal",
            "--bg",
            "none",
            "--preview",
        ],
    );
    assert_eq!(candidate["preview"], true);
    assert_eq!(candidate["role"], "url");
    assert_eq!(candidate["style"]["fg"], "#008080");
    assert!(candidate["style"]["bg"].is_null());
    assert_eq!(candidate["theme"]["name"], "rose-pink");
    cli(&config)
        .args([
            "--color",
            "always",
            "theme",
            "set",
            "url",
            "--fg",
            "teal",
            "--preview",
        ])
        .assert()
        .success()
        .stdout(contains("not saved"))
        .stdout(contains("\x1b[38;2;0;128;128mhttps://example.org"));
    assert_eq!(read_to_string(&config).unwrap(), initial);
    assert_eq!(json(&config, &["theme", "preview"]), before);
}

#[test]
fn preview_does_not_create_a_missing_config() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    cargo_bin_cmd!("hubuum-cli")
        .env("XDG_CONFIG_HOME", dir.path())
        .args(["theme", "set", "key", "--fg", "blue", "--preview"])
        .assert()
        .success();
    assert!(!config.exists());
    assert!(!dir.path().join(".hubuum_cli/config.toml").exists());
}

#[test]
fn saves_partial_edits_per_theme_and_reloads_them() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    write(&config, "[output]\ntheme = 'rose-pink'\npadding = 3\n[output.theme_overrides.ocean-blue.url]\nfg = 'ansi:blue'\n").unwrap();
    let saved = json(
        &config,
        &["theme", "set", "url", "--fg", "teal", "--bg", "ansi256:235"],
    );
    assert_eq!(saved["theme"], "rose-pink");
    assert_eq!(saved["style"]["fg"], "#008080");
    assert_eq!(saved["style"]["bg"], "ansi256:235");
    let foreground = json(
        &config,
        &["theme", "set", "--role", "url", "--fg", "default"],
    );
    assert!(foreground["style"]["fg"].is_null());
    assert_eq!(foreground["style"]["bg"], "ansi256:235");
    json(&config, &["theme", "set", "url", "--bg", "none"]);
    let loaded = json(&config, &["theme", "preview"]);
    assert!(loaded["roles"]["url"]["fg"].is_null());
    assert!(loaded["roles"]["url"]["bg"].is_null());
    let stored: toml::Value = toml::from_str(&read_to_string(&config).unwrap()).unwrap();
    assert_eq!(stored["output"]["padding"].as_integer(), Some(3));
    assert_eq!(
        stored["output"]["theme_overrides"]["rose-pink"]["url"]["fg"].as_str(),
        Some("default")
    );
    let other = json(&config, &["theme", "preview", "ocean-blue"]);
    assert_eq!(other["roles"]["url"]["fg"], "ansi:blue");
}

#[test]
fn network_overrides_reach_addresses_and_preserve_null_dimming() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    write(&config, "").unwrap();
    json(&config, &["theme", "set", "network", "--fg", "#008080"]);
    json(
        &config,
        &[
            "theme",
            "set",
            "null_like",
            "--fg",
            "none",
            "--bg",
            "default",
        ],
    );
    let shown = json(&config, &["theme", "show"]);
    let roles = shown["roles"].as_array().unwrap();
    for role in ["Network", "Ipv4", "Ipv6", "Cidr", "Mac"] {
        let style = roles.iter().find(|entry| entry["role"] == role).unwrap();
        assert_eq!(style["foreground"], "#008080");
    }
    let nulls = roles
        .iter()
        .find(|entry| entry["role"] == "NullLike")
        .unwrap();
    assert_eq!(nulls["dimmed"], true);
}

#[test]
fn invalid_edits_leave_config_unchanged() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let initial = "[output]\ntheme = 'rose-pink'\n";
    write(&config, initial).unwrap();
    for args in [
        vec!["theme", "set", "url"],
        vec!["theme", "set", "--fg", "teal"],
        vec!["theme", "set", "typo", "--fg", "teal"],
        vec!["theme", "set", "url", "--fg", "ansi256:256"],
        vec!["theme", "set", "url", "--fg", "red", "--bg", "invalid"],
        vec!["theme", "set", "url", "--fg", "invalid", "--preview"],
        vec!["theme", "set", "url", "extra", "--fg", "teal"],
        vec!["theme", "set", "url", "--role", "key", "--fg", "teal"],
    ] {
        cli(&config).args(args).assert().failure();
        assert_eq!(read_to_string(&config).unwrap(), initial);
    }
}

#[test]
fn custom_theme_edits_preserve_styles_and_theme_file() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let theme_file = dir.path().join("themes.toml");
    let definition = "[[theme]]\nname = 'custom'\n[theme.roles]\nkey = { fg = 'ansi:red', bg = 'ansi:blue', bold = true }\n";
    write(&theme_file, definition).unwrap();
    write(
        &config,
        format!(
            "[output]\ntheme = 'custom'\ntheme_file = '{}'\n",
            theme_file.display()
        ),
    )
    .unwrap();
    let edited = json(&config, &["theme", "set", "key", "--fg", "default"]);
    assert!(edited["style"]["fg"].is_null());
    assert_eq!(edited["style"]["bg"], "ansi:blue");
    assert_eq!(edited["style"]["bold"], true);
    assert_eq!(read_to_string(&theme_file).unwrap(), definition);
    assert_eq!(
        json(&config, &["theme", "preview"])["roles"]["key"],
        edited["style"]
    );
}

#[test]
fn saving_updates_the_running_session_but_preview_does_not() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    write(&config, "[output]\ntheme = 'rose-pink'\n").unwrap();
    let script = dir.path().join("theme.hubuum");
    write(&script, "theme set url --fg teal --bg none\ntheme preview --output json > saved.json\ntheme set url --fg red --preview\ntheme preview --output json > after-preview.json\n").unwrap();
    cli(&config)
        .current_dir(dir.path())
        .arg("script")
        .arg(&script)
        .assert()
        .success();
    let saved: Value =
        serde_json::from_str(&read_to_string(dir.path().join("saved.json")).unwrap()).unwrap();
    let after: Value =
        serde_json::from_str(&read_to_string(dir.path().join("after-preview.json")).unwrap())
            .unwrap();
    assert_eq!(saved["roles"]["url"]["fg"], "#008080");
    assert_eq!(saved, after);
}
