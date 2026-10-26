use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ColorSpec, RoleStyle, ThemeError};

/// A color edit distinguishes clearing a channel from leaving it unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorOverride {
    Default,
    Color(ColorSpec),
}

impl FromStr for ColorOverride {
    type Err = ThemeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "none" | "default" => Ok(Self::Default),
            "teal" => Ok(Self::Color(ColorSpec::rgb(0, 128, 128))),
            color => color.parse().or_else(|_| format!("ansi:{color}").parse())
                .map(Self::Color)
                .map_err(|_| ThemeError::InvalidColor(format!(
                    "'{value}'; use #rrggbb, ansi:<name>, ansi256:<0-255>, an ANSI name, teal, none, or default"
                ))),
        }
    }
}

impl Display for ColorOverride {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Default => f.write_str("default"),
            Self::Color(color) => Display::fmt(color, f),
        }
    }
}

impl Serialize for ColorOverride {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ColorOverride {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(DeserializeError::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoleStyleOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    fg: Option<ColorOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg: Option<ColorOverride>,
}

impl RoleStyleOverride {
    pub const fn new(fg: Option<ColorOverride>, bg: Option<ColorOverride>) -> Self {
        Self { fg, bg }
    }

    pub fn merge(&mut self, patch: Self) {
        self.fg = patch.fg.or(self.fg);
        self.bg = patch.bg.or(self.bg);
    }

    pub fn apply(self, mut style: RoleStyle) -> RoleStyle {
        for (channel, patch) in [(&mut style.fg, self.fg), (&mut style.bg, self.bg)] {
            if let Some(color) = patch {
                *channel = match color {
                    ColorOverride::Default => None,
                    ColorOverride::Color(color) => Some(color),
                };
            }
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{builtin_themes, ThemeRole};

    #[test]
    fn color_edits_validate_and_round_trip() {
        for (input, canonical) in [
            ("none", "default"),
            ("DEFAULT", "default"),
            ("teal", "#008080"),
            ("cyan", "ansi:cyan"),
            ("ansi:bright-red", "ansi:bright-red"),
            ("#Aa00ff", "#aa00ff"),
            ("ansi256:255", "ansi256:255"),
        ] {
            let color: ColorOverride = input.parse().unwrap();
            assert_eq!(color.to_string(), canonical);
            let patch = RoleStyleOverride::new(Some(color), None);
            let serialized = toml::to_string(&patch).unwrap();
            assert_eq!(
                toml::from_str::<RoleStyleOverride>(&serialized).unwrap(),
                patch
            );
            assert!(!serialized.contains("bg"));
        }
        for input in [
            "",
            "pinkish",
            "#fff",
            "#gg0000",
            "ansi256:256",
            "ansi256:-1",
        ] {
            assert!(input.parse::<ColorOverride>().is_err(), "{input}");
        }
        assert!(toml::from_str::<RoleStyleOverride>("foreground = 'teal'").is_err());
    }

    #[test]
    fn partial_edits_preserve_other_colors_and_effects() {
        let base = RoleStyle::new(
            Some(ColorSpec::rgb(1, 2, 3)),
            Some(ColorSpec::ansi256(20)),
            true,
        )
        .with_dimmed(true);
        let mut patch = RoleStyleOverride::new(Some(ColorOverride::Default), None);
        let edited = patch.apply(base);
        assert_eq!(edited.fg, None);
        assert_eq!(edited.bg, base.bg);
        assert!(edited.bold && edited.dimmed);
        patch.merge(RoleStyleOverride::new(None, Some(ColorOverride::Default)));
        assert_eq!(
            patch.apply(base),
            RoleStyle::new(None, None, true).with_dimmed(true)
        );
    }

    #[test]
    fn overrides_follow_role_inheritance_in_every_palette() {
        let teal = ColorSpec::rgb(0, 128, 128);
        let overrides = BTreeMap::from([
            (
                ThemeRole::Command,
                RoleStyleOverride::new(Some(ColorOverride::Color(teal)), None),
            ),
            (
                ThemeRole::Network,
                RoleStyleOverride::new(None, Some(ColorOverride::Default)),
            ),
            (
                ThemeRole::Url,
                RoleStyleOverride::new(None, Some(ColorOverride::Default)),
            ),
            (
                ThemeRole::NullLike,
                RoleStyleOverride::new(Some(ColorOverride::Default), None),
            ),
        ]);
        for mut theme in builtin_themes() {
            let heading = theme.role_style(ThemeRole::Heading);
            theme.apply_role_overrides(&overrides);
            for role in [
                ThemeRole::Network,
                ThemeRole::Ipv4,
                ThemeRole::Ipv6,
                ThemeRole::Cidr,
                ThemeRole::Mac,
                ThemeRole::Url,
            ] {
                assert_eq!(
                    theme.role_style(role).fg,
                    Some(teal),
                    "{} {role}",
                    theme.name
                );
            }
            assert_eq!(theme.role_style(ThemeRole::Heading), heading);
            assert!(theme.role_style(ThemeRole::NullLike).dimmed);
        }
    }

    #[test]
    fn role_names_match_serialized_config_keys() {
        for role in ThemeRole::ALL {
            assert_eq!(role.to_string().parse::<ThemeRole>().unwrap(), role);
            let encoded = toml::to_string(&BTreeMap::from([(role, "style")])).unwrap();
            assert!(encoded.starts_with(role.as_str()));
        }
        assert!("typo".parse::<ThemeRole>().is_err());
    }
}
