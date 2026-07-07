use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use anstyle::{Ansi256Color, AnsiColor, Color, RgbColor, Style};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_THEME: &str = "hubuum-dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeRole {
    Error,
    Warning,
    Muted,
    Prompt,
    Heading,
    Command,
    TableBand,
}

impl ThemeRole {
    pub const ALL: [ThemeRole; 7] = [
        ThemeRole::Error,
        ThemeRole::Warning,
        ThemeRole::Muted,
        ThemeRole::Prompt,
        ThemeRole::Heading,
        ThemeRole::Command,
        ThemeRole::TableBand,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub display_name: String,
    pub license: ThemeLicense,
    pub roles: BTreeMap<ThemeRole, RoleStyle>,
}

impl Theme {
    pub fn style(&self, role: ThemeRole) -> Style {
        self.roles
            .get(&role)
            .copied()
            .unwrap_or_default()
            .into_style()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeLicense {
    pub name: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleStyle {
    #[serde(default)]
    pub fg: Option<ColorSpec>,
    #[serde(default)]
    pub bg: Option<ColorSpec>,
    #[serde(default)]
    pub bold: bool,
}

impl RoleStyle {
    pub const fn new(fg: Option<ColorSpec>, bg: Option<ColorSpec>, bold: bool) -> Self {
        Self { fg, bg, bold }
    }

    pub fn into_style(self) -> Style {
        let mut style = Style::new()
            .fg_color(self.fg.map(ColorSpec::into_color))
            .bg_color(self.bg.map(ColorSpec::into_color));
        if self.bold {
            style = style.bold();
        }
        style
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpec {
    Ansi(AnsiColor),
    Ansi256(u8),
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::Rgb(red, green, blue)
    }

    pub const fn ansi(color: AnsiColor) -> Self {
        Self::Ansi(color)
    }

    pub const fn ansi256(color: u8) -> Self {
        Self::Ansi256(color)
    }

    pub fn into_color(self) -> Color {
        match self {
            ColorSpec::Ansi(color) => Color::Ansi(color),
            ColorSpec::Ansi256(color) => Color::Ansi256(Ansi256Color(color)),
            ColorSpec::Rgb(red, green, blue) => Color::Rgb(RgbColor(red, green, blue)),
        }
    }
}

impl Serialize for ColorSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for ColorSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorSpec::Ansi(color) => write!(f, "ansi:{}", ansi_name(*color)),
            ColorSpec::Ansi256(color) => write!(f, "ansi256:{color}"),
            ColorSpec::Rgb(red, green, blue) => write!(f, "#{red:02x}{green:02x}{blue:02x}"),
        }
    }
}

impl FromStr for ColorSpec {
    type Err = ThemeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(hex) = value.strip_prefix('#') {
            return parse_hex_color(hex);
        }
        if let Some(name) = value.strip_prefix("ansi:") {
            return parse_ansi_color(name).map(ColorSpec::Ansi);
        }
        if let Some(number) = value.strip_prefix("ansi256:") {
            return Ok(ColorSpec::Ansi256(number.parse().map_err(|_| {
                ThemeError::InvalidColor(format!("invalid ansi256 color '{value}'"))
            })?));
        }
        Err(ThemeError::InvalidColor(format!(
            "invalid color '{value}', expected #rrggbb, ansi:<name>, or ansi256:<0-255>"
        )))
    }
}

#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    themes: BTreeMap<String, Theme>,
}

impl ThemeCatalog {
    pub fn names(&self) -> Vec<&str> {
        self.themes.keys().map(String::as_str).collect()
    }

    pub fn themes(&self) -> impl Iterator<Item = &Theme> {
        self.themes.values()
    }

    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name)
    }
}

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("Theme '{0}' was not found")]
    UnknownTheme(String),
    #[error("Theme '{0}' is defined more than once")]
    DuplicateTheme(String),
    #[error("Theme '{0}' has an invalid name; use lowercase letters, numbers, and dashes")]
    InvalidThemeName(String),
    #[error("Invalid color: {0}")]
    InvalidColor(String),
    #[error("Theme '{theme}' is missing role '{role:?}'")]
    MissingRole { theme: String, role: ThemeRole },
    #[error("Theme '{theme}' inherits unknown theme '{base}'")]
    UnknownBaseTheme { theme: String, base: String },
    #[error("Theme inheritance cycle includes '{0}'")]
    InheritanceCycle(String),
    #[error("Theme file could not be read: {0}")]
    Read(String),
    #[error("Theme file could not be parsed: {0}")]
    Parse(String),
}

pub fn paint(theme: &Theme, role: ThemeRole, text: impl AsRef<str>) -> String {
    let style = theme.style(role);
    format!(
        "{}{}{}",
        style.render(),
        text.as_ref(),
        style.render_reset()
    )
}

pub fn theme_names() -> Vec<String> {
    builtin_themes()
        .into_iter()
        .map(|theme| theme.name)
        .collect()
}

pub fn builtin_themes() -> Vec<Theme> {
    vec![
        theme(
            "hubuum-dark",
            "Hubuum Dark",
            first_party_license(),
            &[
                (ThemeRole::Error, role_ansi(AnsiColor::Red, true)),
                (ThemeRole::Warning, role_ansi(AnsiColor::Yellow, true)),
                (ThemeRole::Muted, role_ansi(AnsiColor::BrightBlack, false)),
                (ThemeRole::Prompt, role_ansi(AnsiColor::Cyan, true)),
                (ThemeRole::Heading, role_ansi(AnsiColor::Green, true)),
                (ThemeRole::Command, role_ansi(AnsiColor::Green, false)),
                (
                    ThemeRole::TableBand,
                    RoleStyle::new(None, Some(ColorSpec::ansi256(236)), false),
                ),
            ],
        ),
        theme(
            "hubuum-light",
            "Hubuum Light",
            first_party_license(),
            &[
                (ThemeRole::Error, role_rgb(0xb0, 0x00, 0x20, true)),
                (ThemeRole::Warning, role_rgb(0x8a, 0x5a, 0x00, true)),
                (ThemeRole::Muted, role_rgb(0x66, 0x66, 0x66, false)),
                (ThemeRole::Prompt, role_rgb(0x00, 0x66, 0x7a, true)),
                (ThemeRole::Heading, role_rgb(0x00, 0x6d, 0x3a, true)),
                (ThemeRole::Command, role_rgb(0x00, 0x7a, 0x3d, false)),
                (
                    ThemeRole::TableBand,
                    RoleStyle::new(None, Some(ColorSpec::rgb(0xf1, 0xf3, 0xf4)), false),
                ),
            ],
        ),
        catppuccin_mocha(),
        catppuccin_latte(),
        solarized_dark(),
        solarized_light(),
    ]
}

pub fn catalog(theme_file: Option<&Path>) -> Result<ThemeCatalog, ThemeError> {
    let mut themes = builtin_themes()
        .into_iter()
        .map(|theme| (theme.name.clone(), theme))
        .collect::<BTreeMap<_, _>>();
    if let Some(path) = theme_file {
        for theme in load_theme_file(path, &themes)? {
            if themes.insert(theme.name.clone(), theme.clone()).is_some() {
                return Err(ThemeError::DuplicateTheme(theme.name));
            }
        }
    }
    Ok(ThemeCatalog { themes })
}

pub fn resolve_theme(name: &str, theme_file: Option<&Path>) -> Result<Theme, ThemeError> {
    catalog(theme_file)?
        .get(name)
        .cloned()
        .ok_or_else(|| ThemeError::UnknownTheme(name.to_string()))
}

pub fn load_theme_file(
    path: &Path,
    builtin_base: &BTreeMap<String, Theme>,
) -> Result<Vec<Theme>, ThemeError> {
    let text = std::fs::read_to_string(path).map_err(|err| ThemeError::Read(err.to_string()))?;
    let file: ThemeFile =
        toml::from_str(&text).map_err(|err| ThemeError::Parse(err.to_string()))?;
    build_custom_themes(file.theme, builtin_base)
}

#[derive(Debug, Deserialize)]
struct ThemeFile {
    #[serde(default)]
    theme: Vec<ThemeDefinition>,
}

#[derive(Debug, Deserialize)]
struct ThemeDefinition {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    inherits: Option<String>,
    #[serde(default)]
    roles: BTreeMap<ThemeRole, RoleStyle>,
}

fn build_custom_themes(
    definitions: Vec<ThemeDefinition>,
    builtin_base: &BTreeMap<String, Theme>,
) -> Result<Vec<Theme>, ThemeError> {
    let mut definitions_by_name = HashMap::new();
    for definition in definitions {
        validate_theme_name(&definition.name)?;
        let name = definition.name.clone();
        if definitions_by_name
            .insert(name.clone(), definition)
            .is_some()
        {
            return Err(ThemeError::DuplicateTheme(name));
        }
    }

    let mut built = BTreeMap::new();
    for name in definitions_by_name.keys() {
        build_custom_theme(
            name,
            &definitions_by_name,
            builtin_base,
            &mut built,
            &mut Vec::new(),
        )?;
    }
    Ok(built.into_values().collect())
}

fn build_custom_theme(
    name: &str,
    definitions: &HashMap<String, ThemeDefinition>,
    builtin_base: &BTreeMap<String, Theme>,
    built: &mut BTreeMap<String, Theme>,
    stack: &mut Vec<String>,
) -> Result<Theme, ThemeError> {
    if let Some(theme) = built.get(name) {
        return Ok(theme.clone());
    }
    if stack.iter().any(|entry| entry == name) {
        return Err(ThemeError::InheritanceCycle(name.to_string()));
    }
    let definition = definitions
        .get(name)
        .ok_or_else(|| ThemeError::UnknownTheme(name.to_string()))?;
    stack.push(name.to_string());

    let base_name = definition
        .inherits
        .as_deref()
        .unwrap_or(DEFAULT_THEME)
        .to_string();
    let mut roles = if let Some(theme) = builtin_base.get(&base_name) {
        theme.roles.clone()
    } else if definitions.contains_key(&base_name) {
        build_custom_theme(&base_name, definitions, builtin_base, built, stack)?.roles
    } else {
        return Err(ThemeError::UnknownBaseTheme {
            theme: name.to_string(),
            base: base_name,
        });
    };

    for (role, style) in &definition.roles {
        roles.insert(*role, *style);
    }

    let theme = Theme {
        name: definition.name.clone(),
        display_name: definition
            .display_name
            .clone()
            .unwrap_or_else(|| definition.name.clone()),
        license: ThemeLicense {
            name: "User provided".to_string(),
            source: None,
        },
        roles,
    };
    validate_theme(&theme)?;
    built.insert(theme.name.clone(), theme.clone());
    stack.pop();
    Ok(theme)
}

fn validate_theme(theme: &Theme) -> Result<(), ThemeError> {
    validate_theme_name(&theme.name)?;
    for role in ThemeRole::ALL {
        if !theme.roles.contains_key(&role) {
            return Err(ThemeError::MissingRole {
                theme: theme.name.clone(),
                role,
            });
        }
    }
    Ok(())
}

fn validate_theme_name(name: &str) -> Result<(), ThemeError> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Ok(());
    }
    Err(ThemeError::InvalidThemeName(name.to_string()))
}

fn theme(
    name: &str,
    display_name: &str,
    license: ThemeLicense,
    roles: &[(ThemeRole, RoleStyle)],
) -> Theme {
    let roles = roles.iter().copied().collect::<BTreeMap<_, _>>();
    let theme = Theme {
        name: name.to_string(),
        display_name: display_name.to_string(),
        license,
        roles,
    };
    validate_theme(&theme).expect("built-in theme should be complete");
    theme
}

fn catppuccin_mocha() -> Theme {
    theme(
        "catppuccin-mocha",
        "Catppuccin Mocha",
        ThemeLicense {
            name: "MIT".to_string(),
            source: Some("https://github.com/catppuccin/catppuccin/blob/main/LICENSE".to_string()),
        },
        &[
            (ThemeRole::Error, role_hex(0xf3, 0x8b, 0xa8, true)),
            (ThemeRole::Warning, role_hex(0xf9, 0xe2, 0xaf, true)),
            (ThemeRole::Muted, role_hex(0x6c, 0x70, 0x86, false)),
            (ThemeRole::Prompt, role_hex(0x89, 0xdc, 0xeb, true)),
            (ThemeRole::Heading, role_hex(0xa6, 0xe3, 0xa1, true)),
            (ThemeRole::Command, role_hex(0xa6, 0xe3, 0xa1, false)),
            (
                ThemeRole::TableBand,
                RoleStyle::new(None, Some(ColorSpec::rgb(0x24, 0x25, 0x37)), false),
            ),
        ],
    )
}

fn catppuccin_latte() -> Theme {
    theme(
        "catppuccin-latte",
        "Catppuccin Latte",
        ThemeLicense {
            name: "MIT".to_string(),
            source: Some("https://github.com/catppuccin/catppuccin/blob/main/LICENSE".to_string()),
        },
        &[
            (ThemeRole::Error, role_hex(0xd2, 0x0f, 0x39, true)),
            (ThemeRole::Warning, role_hex(0xdf, 0x8e, 0x1d, true)),
            (ThemeRole::Muted, role_hex(0x9c, 0xa0, 0xb0, false)),
            (ThemeRole::Prompt, role_hex(0x04, 0xa5, 0xe5, true)),
            (ThemeRole::Heading, role_hex(0x40, 0xa0, 0x2b, true)),
            (ThemeRole::Command, role_hex(0x40, 0xa0, 0x2b, false)),
            (
                ThemeRole::TableBand,
                RoleStyle::new(None, Some(ColorSpec::rgb(0xe6, 0xe9, 0xef)), false),
            ),
        ],
    )
}

fn solarized_dark() -> Theme {
    theme(
        "solarized-dark",
        "Solarized Dark",
        ThemeLicense {
            name: "MIT".to_string(),
            source: Some(
                "https://github.com/altercation/solarized/blob/master/LICENSE".to_string(),
            ),
        },
        &[
            (ThemeRole::Error, role_hex(0xdc, 0x32, 0x2f, true)),
            (ThemeRole::Warning, role_hex(0xb5, 0x89, 0x00, true)),
            (ThemeRole::Muted, role_hex(0x58, 0x6e, 0x75, false)),
            (ThemeRole::Prompt, role_hex(0x2a, 0xa1, 0x98, true)),
            (ThemeRole::Heading, role_hex(0x85, 0x99, 0x00, true)),
            (ThemeRole::Command, role_hex(0x85, 0x99, 0x00, false)),
            (
                ThemeRole::TableBand,
                RoleStyle::new(None, Some(ColorSpec::rgb(0x07, 0x36, 0x42)), false),
            ),
        ],
    )
}

fn solarized_light() -> Theme {
    theme(
        "solarized-light",
        "Solarized Light",
        ThemeLicense {
            name: "MIT".to_string(),
            source: Some(
                "https://github.com/altercation/solarized/blob/master/LICENSE".to_string(),
            ),
        },
        &[
            (ThemeRole::Error, role_hex(0xdc, 0x32, 0x2f, true)),
            (ThemeRole::Warning, role_hex(0xb5, 0x89, 0x00, true)),
            (ThemeRole::Muted, role_hex(0x93, 0xa1, 0xa1, false)),
            (ThemeRole::Prompt, role_hex(0x2a, 0xa1, 0x98, true)),
            (ThemeRole::Heading, role_hex(0x85, 0x99, 0x00, true)),
            (ThemeRole::Command, role_hex(0x85, 0x99, 0x00, false)),
            (
                ThemeRole::TableBand,
                RoleStyle::new(None, Some(ColorSpec::rgb(0xee, 0xe8, 0xd5)), false),
            ),
        ],
    )
}

fn first_party_license() -> ThemeLicense {
    ThemeLicense {
        name: "MIT".to_string(),
        source: Some("https://github.com/unioslo/hubuum-cli-rs/blob/main/LICENSE".to_string()),
    }
}

fn role_ansi(color: AnsiColor, bold: bool) -> RoleStyle {
    RoleStyle::new(Some(ColorSpec::ansi(color)), None, bold)
}

fn role_rgb(red: u8, green: u8, blue: u8, bold: bool) -> RoleStyle {
    RoleStyle::new(Some(ColorSpec::rgb(red, green, blue)), None, bold)
}

fn role_hex(red: u8, green: u8, blue: u8, bold: bool) -> RoleStyle {
    role_rgb(red, green, blue, bold)
}

fn parse_hex_color(hex: &str) -> Result<ColorSpec, ThemeError> {
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ThemeError::InvalidColor(format!(
            "invalid hex color '#{hex}'"
        )));
    }
    let red = u8::from_str_radix(&hex[0..2], 16).expect("hex was validated");
    let green = u8::from_str_radix(&hex[2..4], 16).expect("hex was validated");
    let blue = u8::from_str_radix(&hex[4..6], 16).expect("hex was validated");
    Ok(ColorSpec::Rgb(red, green, blue))
}

fn parse_ansi_color(name: &str) -> Result<AnsiColor, ThemeError> {
    match name {
        "black" => Ok(AnsiColor::Black),
        "red" => Ok(AnsiColor::Red),
        "green" => Ok(AnsiColor::Green),
        "yellow" => Ok(AnsiColor::Yellow),
        "blue" => Ok(AnsiColor::Blue),
        "magenta" => Ok(AnsiColor::Magenta),
        "cyan" => Ok(AnsiColor::Cyan),
        "white" => Ok(AnsiColor::White),
        "bright-black" | "bright_black" => Ok(AnsiColor::BrightBlack),
        "bright-red" | "bright_red" => Ok(AnsiColor::BrightRed),
        "bright-green" | "bright_green" => Ok(AnsiColor::BrightGreen),
        "bright-yellow" | "bright_yellow" => Ok(AnsiColor::BrightYellow),
        "bright-blue" | "bright_blue" => Ok(AnsiColor::BrightBlue),
        "bright-magenta" | "bright_magenta" => Ok(AnsiColor::BrightMagenta),
        "bright-cyan" | "bright_cyan" => Ok(AnsiColor::BrightCyan),
        "bright-white" | "bright_white" => Ok(AnsiColor::BrightWhite),
        _ => Err(ThemeError::InvalidColor(format!(
            "unknown ANSI color '{name}'"
        ))),
    }
}

fn ansi_name(color: AnsiColor) -> &'static str {
    match color {
        AnsiColor::Black => "black",
        AnsiColor::Red => "red",
        AnsiColor::Green => "green",
        AnsiColor::Yellow => "yellow",
        AnsiColor::Blue => "blue",
        AnsiColor::Magenta => "magenta",
        AnsiColor::Cyan => "cyan",
        AnsiColor::White => "white",
        AnsiColor::BrightBlack => "bright-black",
        AnsiColor::BrightRed => "bright-red",
        AnsiColor::BrightGreen => "bright-green",
        AnsiColor::BrightYellow => "bright-yellow",
        AnsiColor::BrightBlue => "bright-blue",
        AnsiColor::BrightMagenta => "bright-magenta",
        AnsiColor::BrightCyan => "bright-cyan",
        AnsiColor::BrightWhite => "bright-white",
    }
}

pub fn assert_external_palettes_are_mit() {
    let allowed = HashSet::from(["hubuum-dark", "hubuum-light"]);
    for theme in builtin_themes() {
        if allowed.contains(theme.name.as_str()) {
            continue;
        }
        assert_eq!(
            theme.license.name, "MIT",
            "{} must be MIT licensed",
            theme.name
        );
        assert!(
            theme.license.source.as_deref().is_some_and(|source| {
                source.contains("catppuccin") || source.contains("solarized")
            }),
            "{} must have a documented MIT source",
            theme.name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_define_every_role() {
        for theme in builtin_themes() {
            for role in ThemeRole::ALL {
                assert!(
                    theme.roles.contains_key(&role),
                    "{} missing {role:?}",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn external_builtin_palettes_are_mit() {
        assert_external_palettes_are_mit();
    }

    #[test]
    fn custom_theme_inherits_and_overrides() {
        let builtins = builtin_themes()
            .into_iter()
            .map(|theme| (theme.name.clone(), theme))
            .collect::<BTreeMap<_, _>>();
        let custom = r##"
            [[theme]]
            name = "night-ops"
            display_name = "Night Ops"
            inherits = "hubuum-dark"

            [theme.roles]
            command = { fg = "#7ee787" }
            heading = { fg = "ansi:cyan", bold = true }
            table_band = { bg = "ansi256:235" }
        "##;
        let file: ThemeFile = toml::from_str(custom).expect("custom theme parses");
        let themes = build_custom_themes(file.theme, &builtins).expect("custom theme builds");
        let theme = themes
            .iter()
            .find(|theme| theme.name == "night-ops")
            .expect("night-ops");
        assert_eq!(
            theme
                .roles
                .get(&ThemeRole::Command)
                .and_then(|role| role.fg),
            Some(ColorSpec::rgb(0x7e, 0xe7, 0x87))
        );
        assert_eq!(
            theme.roles.get(&ThemeRole::Error),
            builtins
                .get("hubuum-dark")
                .expect("builtin")
                .roles
                .get(&ThemeRole::Error)
        );
    }

    #[test]
    fn custom_theme_rejects_duplicate_names() {
        let builtins = builtin_themes()
            .into_iter()
            .map(|theme| (theme.name.clone(), theme))
            .collect::<BTreeMap<_, _>>();
        let custom = r#"
            [[theme]]
            name = "dup"

            [[theme]]
            name = "dup"
        "#;
        let file: ThemeFile = toml::from_str(custom).expect("custom theme parses");
        assert!(matches!(
            build_custom_themes(file.theme, &builtins),
            Err(ThemeError::DuplicateTheme(_))
        ));
    }

    #[test]
    fn color_specs_parse() {
        assert_eq!(
            "#aabbcc".parse::<ColorSpec>().unwrap(),
            ColorSpec::rgb(0xaa, 0xbb, 0xcc)
        );
        assert_eq!(
            "ansi:bright-green".parse::<ColorSpec>().unwrap(),
            ColorSpec::ansi(AnsiColor::BrightGreen)
        );
        assert_eq!(
            "ansi256:236".parse::<ColorSpec>().unwrap(),
            ColorSpec::ansi256(236)
        );
        assert!("not-a-color".parse::<ColorSpec>().is_err());
    }
}
