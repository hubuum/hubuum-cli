use hubuum_client::FilterOperator;
use log::{trace, warn};
use strum::IntoEnumIterator;

use crate::commandlist::CommandList;

pub fn bool(_cmdlist: &CommandList, _prefix: &str, _parts: &[String]) -> Vec<String> {
    vec!["true".to_string(), "false".to_string()]
}

pub fn groups(cmdlist: &CommandList, prefix: &str, _parts: &[String]) -> Vec<String> {
    trace!("Autocompleting groups with prefix: {prefix}");
    let mut cmd = cmdlist.client().groups().find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "groupname",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }
    match cmd.execute() {
        Ok(groups) => groups.into_iter().map(|g| g.groupname).collect(),
        Err(_) => {
            warn!("Failed to fetch groups for autocomplete");
            Vec::new()
        }
    }
}

pub fn classes(cmdlist: &CommandList, prefix: &str, _parts: &[String]) -> Vec<String> {
    trace!("Autocompleting classes with prefix: {prefix}");
    let mut cmd = cmdlist.client().classes().find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }
    match cmd.execute() {
        Ok(classes) => classes.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch classes for autocomplete");
            Vec::new()
        }
    }
}

pub fn namespaces(cmdlist: &CommandList, prefix: &str, _parts: &[String]) -> Vec<String> {
    trace!("Autocompleting namespaces with prefix: {prefix}");
    let mut cmd = cmdlist.client().namespaces().find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }
    match cmd.execute() {
        Ok(namespaces) => namespaces.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch namespaces for autocomplete");
            Vec::new()
        }
    }
}

#[allow(dead_code)]
pub fn permissions(_cmdlist: &CommandList, prefix: &str, _parts: &[String]) -> Vec<String> {
    use hubuum_client::types::Permissions;

    trace!("Autocompleting permissions with prefix: {prefix}");

    // Permissions are are an enum, so we can just return the variants that match the prefix.
    Permissions::iter()
        .filter(|p| p.to_string().starts_with(prefix))
        .map(|p| p.to_string())
        .collect()
}

fn objects_from_class_source(
    cmdlist: &CommandList,
    prefix: &str,
    parts: &[String],
    source: &str,
) -> Vec<String> {
    trace!(
        "Autocompleting objects via source '{source}' with prefix: {prefix}"
    );
    let classname = match parts.windows(2).find(|w| w[0] == source) {
        Some(window) => window[1].clone(),
        None => return Vec::new(),
    };

    let class = match cmdlist
        .client()
        .classes()
        .find()
        .add_filter_name_exact(classname)
        .execute_expecting_single_result()
    {
        Ok(ret) => ret,
        Err(_) => {
            warn!("Failed to fetch class from {source} autocomplete");
            return Vec::new();
        }
    };

    let mut cmd = cmdlist.client().objects(class.id).find();

    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }

    match cmd.execute() {
        Ok(objects) => objects.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch objects for autocomplete");
            Vec::new()
        }
    }
}

pub fn objects_from_class(cmdlist: &CommandList, prefix: &str, parts: &[String]) -> Vec<String> {
    objects_from_class_source(cmdlist, prefix, parts, "--class")
}

pub fn objects_from_class_from(
    cmdlist: &CommandList,
    prefix: &str,
    parts: &[String],
) -> Vec<String> {
    objects_from_class_source(cmdlist, prefix, parts, "--class_from")
}

pub fn objects_from_class_to(cmdlist: &CommandList, prefix: &str, parts: &[String]) -> Vec<String> {
    objects_from_class_source(cmdlist, prefix, parts, "--class_to")
}
