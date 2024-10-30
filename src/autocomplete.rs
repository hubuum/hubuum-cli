use log::warn;

use hubuum_client::FilterOperator;

use crate::commandlist::CommandList;

pub fn classes(cmdlist: &CommandList, prefix: &str, _parts: &[String]) -> Vec<String> {
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

pub fn objects_from_class(cmdlist: &CommandList, prefix: &str, parts: &[String]) -> Vec<String> {
    // Find the class name as the value following the --class flag
    let classname = match parts.windows(2).find(|w| w[0] == "--class") {
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
            warn!("Failed to fetch class for autocomplete");
            return Vec::new();
        }
    };

    // Build the command to fetch objects associated with the class
    let mut cmd = cmdlist.client().objects(class.id).find();

    // Add a prefix filter if provided
    if !prefix.is_empty() {
        cmd = cmd.add_filter(
            "name",
            FilterOperator::StartsWith { is_negated: false },
            prefix,
        );
    }

    // Execute the command and collect the object names
    match cmd.execute() {
        Ok(objects) => objects.into_iter().map(|c| c.name).collect(),
        Err(_) => {
            warn!("Failed to fetch objects for autocomplete");
            Vec::new()
        }
    }
}
