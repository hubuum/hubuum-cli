use hubuum_theme::{RoleStyleOverride, ThemeRole};
use std::fs::{create_dir_all, read_to_string, write};
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use toml::map::Map as TomlMap;
use toml::{from_str as parse_toml, to_string_pretty as format_toml, Value as TomlValue};

use crate::domain::{
    CommandAliasDescription, CommandAliasName, CommandAliasTarget, ComputedFieldSet,
};
use crate::errors::AppError;
use crate::models::{
    EmptyResult, ObjectListDataColumns, OutputColor, OutputFormat, Protocol, SessionStorage,
    TableBands, TableHeaders, TableStyle, TableWidth, TableWrap,
};
use crate::theme::available_themes;

use super::{
    descriptor_for_key, get_config_state, ConfigKeyDescriptor, ConfigValueKind, ThemeOverrides,
    UserPreferences,
};

pub fn persist_theme_override(
    theme: &str,
    role: ThemeRole,
    patch: RoleStyleOverride,
) -> Result<PathBuf, AppError> {
    validate_theme_name_config_value(theme)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    let fields =
        TomlValue::try_from(patch).map_err(|error| AppError::ConfigError(error.to_string()))?;
    // Update only supplied channels, leaving other roles and config layers intact.
    for (channel, value) in fields.as_table().expect("role override is a table") {
        set_toml_path(
            &mut root,
            &format!("output.theme_overrides.{theme}.{role}.{channel}"),
            value.clone(),
        )?;
    }
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn set_persisted_value(key: &str, value: &str) -> Result<PathBuf, AppError> {
    if let Some(alias_name) = command_alias_key(key)? {
        let target = CommandAliasTarget::new(value)?;
        return persist_command_alias(&alias_name, &target, None);
    }
    if let Some(class_name) = object_list_class_columns_key(key) {
        return set_persisted_object_list_class_columns(class_name, value);
    }
    if let Some(class_name) = object_class_computed_fields_key(key) {
        return set_persisted_object_class_computed_fields(class_name, value);
    }
    if let Some((class_name, alias)) = object_list_class_alias_key(key) {
        return set_persisted_object_list_class_alias(class_name, alias, value);
    }
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    let parsed = parse_config_value(descriptor, value)?;
    set_toml_path(&mut root, descriptor.key, parsed)?;
    if descriptor.key == "output.object_list_class_aliases" {
        remove_toml_path(&mut root, "output.object_list_class_meta");
    }
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn unset_persisted_value(key: &str) -> Result<PathBuf, AppError> {
    if let Some(alias_name) = command_alias_key(key)? {
        let path = get_config_state().paths.write_target.clone();
        let mut root = read_toml_file_for_update(&path)?;
        remove_toml_path(&mut root, &format!("aliases.{}", alias_name.as_str()));
        write_toml_file(&path, &root)?;
        return Ok(path);
    }
    if object_list_class_columns_key(key).is_some()
        || object_class_computed_fields_key(key).is_some()
        || object_list_class_alias_key(key).is_some()
    {
        let path = get_config_state().paths.write_target.clone();
        let mut root = read_toml_file_for_update(&path)?;
        if let Some((class_name, alias)) = object_list_class_alias_key(key) {
            remove_toml_path(
                &mut root,
                &format!("output.object_list_class_aliases.{class_name}.{alias}"),
            );
            remove_toml_path(
                &mut root,
                &format!("output.object_list_class_meta.{class_name}.{alias}"),
            );
        } else {
            remove_toml_path(&mut root, key);
        }
        write_toml_file(&path, &root)?;
        return Ok(path);
    }
    let descriptor = descriptor_for_key(key)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    remove_toml_path(&mut root, descriptor.key);
    if descriptor.key == "output.object_list_class_aliases" {
        remove_toml_path(&mut root, "output.object_list_class_meta");
    }
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub fn persist_user_preferences(preferences: &UserPreferences) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    merge_user_preferences(&mut root, preferences)?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub(super) fn merge_user_preferences(
    root: &mut TomlValue,
    preferences: &UserPreferences,
) -> Result<(), AppError> {
    let serialized = TomlValue::try_from(preferences)
        .map_err(|error| AppError::ConfigError(error.to_string()))?;
    let preference_sections = serialized.as_table().ok_or_else(|| {
        AppError::ConfigError("Serialized user preferences must be a TOML table".to_string())
    })?;
    let target = root
        .as_table_mut()
        .ok_or_else(|| AppError::ConfigError("Config root is not a TOML table".to_string()))?;

    for section in ["aliases", "completion", "background", "repl", "relations"] {
        if let Some(value) = preference_sections.get(section) {
            target.insert(section.to_string(), value.clone());
        }
    }
    let output = preference_sections
        .get("output")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| {
            AppError::ConfigError("Serialized output preferences must be a TOML table".to_string())
        })?;
    let target_output = target
        .entry("output".to_string())
        .or_insert_with(|| TomlValue::Table(TomlMap::new()))
        .as_table_mut()
        .ok_or_else(|| AppError::ConfigError("Config output must be a TOML table".to_string()))?;
    for (key, value) in output {
        target_output.insert(key.clone(), value.clone());
    }
    target_output.remove("object_list_class_meta");
    Ok(())
}

pub(super) fn read_toml_file_for_update(path: &Path) -> Result<TomlValue, AppError> {
    let contents = match read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(TomlValue::Table(TomlMap::new()));
        }
        Err(error) => {
            return Err(AppError::ConfigError(format!(
                "Could not read config file '{}': {error}",
                path.display()
            )));
        }
    };
    if contents.trim().is_empty() {
        return Ok(TomlValue::Table(TomlMap::new()));
    }
    parse_toml(&contents).map_err(|error| {
        AppError::ConfigError(format!(
            "Could not parse config file '{}': {error}",
            path.display()
        ))
    })
}

pub(super) fn parse_config_value(
    descriptor: &ConfigKeyDescriptor,
    value: &str,
) -> Result<TomlValue, AppError> {
    let value = match descriptor.value_kind {
        ConfigValueKind::String => TomlValue::String(value.to_string()),
        ConfigValueKind::Bool => TomlValue::Boolean(value.parse()?),
        ConfigValueKind::U16 => TomlValue::Integer(value.parse::<u16>()?.into()),
        ConfigValueKind::U64 => TomlValue::Integer(value.parse::<u64>()? as i64),
        ConfigValueKind::PositiveUsize => {
            let limit = value.parse::<NonZeroUsize>().map_err(|_| {
                AppError::InvalidOption(format!("{} must be a positive byte count", descriptor.key))
            })?;
            TomlValue::Integer(i64::try_from(limit.get()).map_err(|_| {
                AppError::InvalidOption(format!(
                    "{} exceeds the TOML integer range",
                    descriptor.key
                ))
            })?)
        }
        ConfigValueKind::I8 => TomlValue::Integer(i64::from(value.parse::<i8>()?)),
        ConfigValueKind::I32 => TomlValue::Integer(i64::from(value.parse::<i32>()?)),
        ConfigValueKind::Protocol => TomlValue::String(
            value
                .parse::<Protocol>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::SessionStorage => TomlValue::String(
            value
                .parse::<SessionStorage>()
                .map_err(|error| AppError::ConfigError(error.to_string()))?
                .to_string(),
        ),
        ConfigValueKind::OutputFormat => {
            TomlValue::String(parse_output_format(value)?.to_string().to_lowercase())
        }
        ConfigValueKind::OutputColor => TomlValue::String(
            value
                .parse::<OutputColor>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::ThemeName => {
            validate_theme_name_config_value(value)?;
            TomlValue::String(value.to_string())
        }
        ConfigValueKind::ThemeOverrides => {
            let overrides: ThemeOverrides =
                parse_toml(value).map_err(|error| AppError::ConfigError(error.to_string()))?;
            TomlValue::try_from(overrides)
                .map_err(|error| AppError::ConfigError(error.to_string()))?
        }
        ConfigValueKind::TableStyle => TomlValue::String(
            value
                .parse::<TableStyle>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableHeaders => TomlValue::String(
            value
                .parse::<TableHeaders>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableWidth => TomlValue::String(
            value
                .parse::<TableWidth>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableWrap => TomlValue::String(
            value
                .parse::<TableWrap>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::TableBands => TomlValue::String(
            value
                .parse::<TableBands>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::EmptyResult => TomlValue::String(
            value
                .parse::<EmptyResult>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::ObjectListDataColumns => TomlValue::String(
            value
                .parse::<ObjectListDataColumns>()
                .map_err(AppError::ConfigError)?
                .to_string(),
        ),
        ConfigValueKind::StringMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::StringListMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::StringNestedListMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::ComputedFieldSetMap => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
        ConfigValueKind::PathList | ConfigValueKind::StringList => TomlValue::Array(
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| TomlValue::String(item.to_string()))
                .collect(),
        ),
        ConfigValueKind::TomlTable => {
            parse_toml(value).map_err(|err| AppError::ConfigError(err.to_string()))?
        }
    };
    Ok(value)
}

pub(super) fn validate_theme_name_config_value(value: &str) -> Result<(), AppError> {
    let catalog = available_themes().map_err(|err| {
        AppError::ConfigError(format!("Could not load configured theme file: {err}"))
    })?;
    if catalog.get(value).is_some() {
        return Ok(());
    }
    Err(AppError::ConfigError(format!(
        "Unknown theme: {value}. Use one of: {}",
        catalog.names().join(", ")
    )))
}

pub(super) fn object_list_class_columns_key(key: &str) -> Option<&str> {
    key.strip_prefix("output.object_list_class_columns.")
        .filter(|class_name| !class_name.is_empty())
}

pub(super) fn command_alias_key(key: &str) -> Result<Option<CommandAliasName>, AppError> {
    let Some(name) = key.strip_prefix("aliases.") else {
        return Ok(None);
    };
    Ok(Some(CommandAliasName::new(name)?))
}

pub fn persist_command_alias(
    alias_name: &CommandAliasName,
    target: &CommandAliasTarget,
    description: Option<&CommandAliasDescription>,
) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    let value = match description {
        Some(description) => TomlValue::Table(TomlMap::from_iter([
            (
                "command".to_string(),
                TomlValue::String(target.as_str().to_string()),
            ),
            (
                "description".to_string(),
                TomlValue::String(description.as_str().to_string()),
            ),
        ])),
        None => TomlValue::String(target.as_str().to_string()),
    };
    set_toml_path(
        &mut root,
        &format!("aliases.{}", alias_name.as_str()),
        value,
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub(super) fn object_class_computed_fields_key(key: &str) -> Option<&str> {
    key.strip_prefix("output.object_class_computed_fields.")
        .filter(|class_name| !class_name.is_empty())
}

pub(super) fn object_list_class_alias_key(key: &str) -> Option<(&str, &str)> {
    let rest = key
        .strip_prefix("output.object_list_class_aliases.")
        .or_else(|| key.strip_prefix("output.object_list_class_meta."))?;
    let (class_name, alias) = rest.split_once('.')?;
    (!class_name.is_empty() && !alias.is_empty()).then_some((class_name, alias))
}

pub(super) fn set_persisted_object_list_class_columns(
    class_name: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    let columns = value
        .split(',')
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .map(|column| TomlValue::String(column.to_string()))
        .collect::<Vec<_>>();
    set_toml_path(
        &mut root,
        &format!("output.object_list_class_columns.{class_name}"),
        TomlValue::Array(columns),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub(super) fn set_persisted_object_class_computed_fields(
    class_name: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let fields =
        ComputedFieldSet::from_values(&[value.to_string()]).map_err(AppError::ConfigError)?;
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    let fields = fields
        .selectors()
        .iter()
        .map(|field| TomlValue::String(field.to_string()))
        .collect();
    set_toml_path(
        &mut root,
        &format!("output.object_class_computed_fields.{class_name}"),
        TomlValue::Array(fields),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub(super) fn set_persisted_object_list_class_alias(
    class_name: &str,
    alias: &str,
    value: &str,
) -> Result<PathBuf, AppError> {
    let path = get_config_state().paths.write_target.clone();
    let mut root = read_toml_file_for_update(&path)?;
    remove_toml_path(
        &mut root,
        &format!("output.object_list_class_meta.{class_name}.{alias}"),
    );
    let selectors = value
        .split(',')
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
        .map(|selector| TomlValue::String(selector.to_string()))
        .collect::<Vec<_>>();
    set_toml_path(
        &mut root,
        &format!("output.object_list_class_aliases.{class_name}.{alias}"),
        TomlValue::Array(selectors),
    )?;
    write_toml_file(&path, &root)?;
    Ok(path)
}

pub(super) fn parse_output_format(value: &str) -> Result<OutputFormat, AppError> {
    match value.to_lowercase().as_str() {
        "json" => Ok(OutputFormat::Json),
        "text" => Ok(OutputFormat::Text),
        _ => Err(AppError::ConfigError(format!(
            "Invalid output format: {value}. Use 'json' or 'text'."
        ))),
    }
}

pub(super) fn set_toml_path(
    root: &mut TomlValue,
    key: &str,
    value: TomlValue,
) -> Result<(), AppError> {
    let mut current = root;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let Some(table) = current.as_table_mut() else {
                return Err(AppError::ConfigError(
                    "Config root is not a TOML table".to_string(),
                ));
            };
            table.insert(part.to_string(), value);
            return Ok(());
        }

        let Some(table) = current.as_table_mut() else {
            return Err(AppError::ConfigError(
                "Config root is not a TOML table".to_string(),
            ));
        };
        current = table
            .entry(part.to_string())
            .or_insert_with(|| TomlValue::Table(TomlMap::new()));
    }
    Ok(())
}

pub(super) fn remove_toml_path(root: &mut TomlValue, key: &str) {
    let parts = key.split('.').collect::<Vec<_>>();
    remove_toml_path_parts(root, &parts);
}

pub(super) fn remove_toml_path_parts(current: &mut TomlValue, parts: &[&str]) -> bool {
    let Some(table) = current.as_table_mut() else {
        return false;
    };

    if parts.len() == 1 {
        table.remove(parts[0]);
        return table.is_empty();
    }

    let should_remove_child = table
        .get_mut(parts[0])
        .map(|child| remove_toml_path_parts(child, &parts[1..]))
        .unwrap_or(false);

    if should_remove_child {
        table.remove(parts[0]);
    }

    table.is_empty()
}

pub(super) fn write_toml_file(path: &Path, root: &TomlValue) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let rendered = format_toml(root).map_err(|err| AppError::ConfigError(err.to_string()))?;
    write(path, rendered)?;
    Ok(())
}
