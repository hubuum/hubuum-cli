use std::collections::HashMap;
use std::str::FromStr;

use hubuum_client::{
    ExportContentType, ExportInclude, ExportIncludeRelatedObject, ExportLimits,
    ExportMissingDataPolicy, ExportRelationContext, ExportRequest, ExportScope, ExportScopeKind,
    ExportTemplateKind, ExportTemplatePatch, ExportTemplateRunRequest,
};

use crate::diagnostics::client_call;
use crate::domain::{ExportTemplateRecord, ObjectReference, ResourceName, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    fetch_query_results, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
    SortFieldSpec,
};

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone)]
pub struct CreateExportTemplateInput {
    pub name: String,
    pub collection: String,
    pub description: String,
    pub content_type: String,
    pub template: String,
}

#[derive(Debug, Clone)]
pub struct UpdateExportTemplateInput {
    pub name: String,
    pub rename: Option<String>,
    pub collection: Option<String>,
    pub description: Option<String>,
    pub template: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ExportScopeInput {
    Collections,
    Classes(ResourceName),
    ObjectsInClass(ResourceName),
    ClassRelations(ResourceName),
    ObjectRelations(ResourceName),
    RelatedObjects(ObjectReference),
}

impl ExportScopeInput {
    fn resolve(&self, gateway: &HubuumGateway) -> Result<ExportScope, AppError> {
        let (kind, class) = match self {
            Self::Collections => {
                return Ok(ExportScope {
                    kind: ExportScopeKind::Collections,
                    class_id: None,
                    object_id: None,
                })
            }
            Self::RelatedObjects(reference) => {
                let class = gateway.class_handle_by_name(reference.class())?;
                let object = client_call("class.object_by_name", || {
                    class.object_by_name(reference.object())
                })?;
                return Ok(ExportScope {
                    kind: ExportScopeKind::RelatedObjects,
                    class_id: Some(class.id()),
                    object_id: Some(object.id()),
                });
            }
            Self::Classes(name) => (ExportScopeKind::Classes, name),
            Self::ObjectsInClass(name) => (ExportScopeKind::ObjectsInClass, name),
            Self::ClassRelations(name) => (ExportScopeKind::ClassRelations, name),
            Self::ObjectRelations(name) => (ExportScopeKind::ObjectRelations, name),
        };
        Ok(ExportScope {
            kind,
            class_id: Some(gateway.class_handle_by_name(class.as_str())?.id()),
            object_id: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AdHocExport {
    scope: ExportScopeInput,
    relation_depth: Option<i32>,
    include_related: Vec<RelatedExport>,
}

impl AdHocExport {
    pub fn new(scope: ExportScopeInput) -> Self {
        Self {
            scope,
            relation_depth: None,
            include_related: Vec::new(),
        }
    }
    pub fn relation_depth(mut self, depth: Option<i32>) -> Self {
        self.relation_depth = depth;
        self
    }
    pub fn include_related(mut self, related: Vec<RelatedExport>) -> Self {
        self.include_related = related;
        self
    }
}

#[derive(Debug, Clone)]
pub enum ExportSource {
    Template {
        name: ResourceName,
        object: Option<ObjectReference>,
    },
    AdHoc(AdHocExport),
}

#[derive(Debug, Clone)]
pub struct RunExportInput {
    source: ExportSource,
    query: Option<String>,
    missing_data_policy: Option<ExportMissingDataPolicy>,
    limits: Option<ExportLimits>,
}

impl RunExportInput {
    pub fn new(source: ExportSource) -> Self {
        Self {
            source,
            query: None,
            missing_data_policy: None,
            limits: None,
        }
    }
    pub fn query(mut self, query: Option<String>) -> Self {
        self.query = query;
        self
    }
    pub fn missing_data_policy(mut self, policy: Option<ExportMissingDataPolicy>) -> Self {
        self.missing_data_policy = policy;
        self
    }
    pub fn limits(mut self, limits: Option<ExportLimits>) -> Self {
        self.limits = limits;
        self
    }
}

#[derive(Debug, Clone)]
pub struct RelatedExport {
    key: String,
    class: ResourceName,
    max_depth: Option<i32>,
}

impl FromStr for RelatedExport {
    type Err = AppError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (key, class, max_depth) = parse_include_related_spec_parts(value)?;
        Ok(Self {
            key,
            class: ResourceName::new(class)?,
            max_depth,
        })
    }
}

/// Parse a single --include-related spec into (key, ExportIncludeRelatedObject).
///
/// Format: `<key>:<class_name>[:<max_depth>]`
/// - `<key>`: arbitrary string (map key for the related object set)
/// - `<class_name>`: class name
/// - `<max_depth>`: optional integer for max traversal depth
///
/// Examples:
/// - `servers:Hosts` → `key="servers", class_id=<Hosts id>, max_depth=None`
/// - `servers:Hosts:3` → `key="servers", class_id=<Hosts id>, max_depth=Some(3)`
impl RelatedExport {
    fn resolve(
        &self,
        gateway: &HubuumGateway,
    ) -> Result<(String, ExportIncludeRelatedObject), AppError> {
        let class_id = gateway.class_handle_by_name(self.class.as_str())?.id();
        Ok((
            self.key.clone(),
            ExportIncludeRelatedObject {
                class_id,
                class_relation_id: None,
                direction: None,
                limit: None,
                max_depth: self.max_depth,
                sort: None,
            },
        ))
    }
}

fn parse_include_related_spec_parts(spec: &str) -> Result<(String, String, Option<i32>), AppError> {
    let parts: Vec<&str> = spec.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(AppError::ParseError(format!(
            "Invalid --include-related spec '{}': expected '<key>:<class_name>[:<max_depth>]'",
            spec
        )));
    }

    let key = parts[0].to_string();
    if key.is_empty() {
        return Err(AppError::ParseError(format!(
            "Invalid --include-related spec '{}': key cannot be empty",
            spec
        )));
    }

    let class_name = parts[1];
    if class_name.is_empty() {
        return Err(AppError::ParseError(format!(
            "Invalid --include-related spec '{}': class name cannot be empty",
            spec
        )));
    }

    let max_depth = if parts.len() >= 3 {
        Some(parts[2].parse::<i32>().map_err(|_| {
            AppError::ParseError(format!(
                "Invalid --include-related spec '{}': max_depth must be an integer",
                spec
            ))
        })?)
    } else {
        None
    };

    Ok((key, class_name.to_string(), max_depth))
}

impl HubuumGateway {
    pub fn list_export_template_names(&self) -> Result<Vec<String>, AppError> {
        Ok(client_call("export_templates.list", || {
            self.client().export_templates().query().list()
        })?
        .into_iter()
        .map(|template| template.name)
        .collect())
    }

    pub fn list_export_templates(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ExportTemplateRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EXPORT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EXPORT_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = fetch_query_results(
            self.client().export_templates().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        if page.items.is_empty() {
            return Ok(PagedResult::empty(page.next_cursor, page.total_count));
        }

        let collectionmap = find_entities_by_ids(
            &self.client().collections(),
            page.items.iter(),
            |template| template.collection_id,
        )?;

        Ok(page.map(|template| ExportTemplateRecord::new(&template, &collectionmap)))
    }

    pub fn export_template(&self, name: &str) -> Result<ExportTemplateRecord, AppError> {
        let template = client_call("export_templates.get_by_name", || {
            self.client().export_templates().get_by_name(name)
        })?;
        let collection = client_call("collections.get", || {
            self.client()
                .collections()
                .get(template.resource().collection_id)
        })?;
        let collectionmap = HashMap::from([(collection.id(), collection.resource().clone())]);

        Ok(ExportTemplateRecord::new(
            template.resource(),
            &collectionmap,
        ))
    }

    pub fn create_export_template(
        &self,
        input: CreateExportTemplateInput,
    ) -> Result<ExportTemplateRecord, AppError> {
        let collection = client_call("collections.get_by_name", || {
            self.client().collections().get_by_name(&input.collection)
        })?;
        let content_type = ExportContentType::from_str(&input.content_type).map_err(|_| {
            AppError::ParseError(format!("Invalid content type: {}", input.content_type))
        })?;

        let template = client_call("export_templates.create_checked.send", || {
            self.client()
                .export_templates()
                .create_checked()
                .collection_id(collection.id())
                .name(input.name)
                .description(input.description)
                .content_type(content_type)
                .template(input.template)
                .kind(ExportTemplateKind::Export)
                .send()
        })?;

        let collectionmap = HashMap::from([(collection.id(), collection.resource().clone())]);
        Ok(ExportTemplateRecord::new(&template, &collectionmap))
    }

    pub fn update_export_template(
        &self,
        input: UpdateExportTemplateInput,
    ) -> Result<ExportTemplateRecord, AppError> {
        let template = client_call("export_templates.get_by_name", || {
            self.client().export_templates().get_by_name(&input.name)
        })?;
        let collection_id = match input.collection {
            Some(collection) => Some(
                client_call("collections.get_by_name", || {
                    self.client().collections().get_by_name(&collection)
                })?
                .id(),
            ),
            None => None,
        };

        let updated = client_call("export_templates.update.send", || {
            self.client()
                .export_templates()
                .update(template.id())
                .params(ExportTemplatePatch {
                    collection_id,
                    name: input.rename,
                    description: input.description,
                    template: input.template,
                    kind: None,
                    scope_kind: None,
                    class_id: None,
                    default_query: None,
                    include: None,
                    relation_context: None,
                    default_missing_data_policy: None,
                    default_limits: None,
                })
                .send()
        })?;

        let collection = client_call("collections.get", || {
            self.client().collections().get(updated.collection_id)
        })?;
        let collectionmap = HashMap::from([(collection.id(), collection.resource().clone())]);
        Ok(ExportTemplateRecord::new(&updated, &collectionmap))
    }

    pub fn delete_export_template(&self, name: &str) -> Result<(), AppError> {
        let template = client_call("export_templates.get_by_name", || {
            self.client().export_templates().get_by_name(name)
        })?;
        client_call("export_templates.delete", || {
            self.client().export_templates().delete(template.id())
        })?;
        Ok(())
    }

    pub fn submit_export(&self, input: RunExportInput) -> Result<TaskRecord, AppError> {
        match input.source {
            ExportSource::Template { name, object } => {
                let template = client_call("export_templates.get_by_name", || {
                    self.client().export_templates().get_by_name(name.as_str())
                })?;
                let object_id = object
                    .map(|reference| {
                        let class = self.class_handle_by_name(reference.class())?;
                        Ok::<_, AppError>(
                            client_call("class.object_by_name", || {
                                class.object_by_name(reference.object())
                            })?
                            .id(),
                        )
                    })
                    .transpose()?;
                let request = ExportTemplateRunRequest {
                    query: input.query,
                    object_id,
                    missing_data_policy: input.missing_data_policy,
                    limits: input.limits,
                };
                Ok(TaskRecord(client_call(
                    "export_templates.submit_export.send",
                    || {
                        self.client()
                            .export_templates()
                            .submit_export(template.id(), request)
                            .send()
                    },
                )?))
            }
            ExportSource::AdHoc(export) => {
                let scope = export.scope.resolve(self)?;
                let related_objects = export
                    .include_related
                    .iter()
                    .map(|related| related.resolve(self))
                    .collect::<Result<HashMap<_, _>, _>>()?;
                let include = (!related_objects.is_empty()).then_some(ExportInclude {
                    related_objects: Some(related_objects),
                });
                let request = ExportRequest {
                    limits: input.limits,
                    missing_data_policy: input.missing_data_policy,
                    query: input.query,
                    scope,
                    include,
                    relation_context: export
                        .relation_depth
                        .map(|depth| ExportRelationContext { depth: Some(depth) }),
                };
                Ok(TaskRecord(client_call("exports.submit.send", || {
                    self.client().exports().submit(request).send()
                })?))
            }
        }
    }
}

pub(crate) const EXPORT_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "collection",
        "collection_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::CollectionNameToId),
    FilterFieldSpec::new(
        "content_type",
        "content_type",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
];

pub(crate) const EXPORT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("collection", "collection_id"),
    SortFieldSpec::new("content_type", "content_type"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_include_related_spec_valid() {
        // Test basic format: key:class_name
        let (key, class_name, max_depth) =
            parse_include_related_spec_parts("servers:Hosts").unwrap();
        assert_eq!(key, "servers");
        assert_eq!(class_name, "Hosts");
        assert_eq!(max_depth, None);

        // Test with max_depth: key:class_name:max_depth
        let (key, class_name, max_depth) =
            parse_include_related_spec_parts("servers:Hosts:3").unwrap();
        assert_eq!(key, "servers");
        assert_eq!(class_name, "Hosts");
        assert_eq!(max_depth, Some(3));

        // Test with complex key
        let (key, class_name, max_depth) =
            parse_include_related_spec_parts("my_servers:ServerClass:5").unwrap();
        assert_eq!(key, "my_servers");
        assert_eq!(class_name, "ServerClass");
        assert_eq!(max_depth, Some(5));
    }

    #[test]
    fn test_parse_include_related_spec_invalid() {
        // Missing class name
        assert!(parse_include_related_spec_parts("servers").is_err());

        // Empty key
        assert!(parse_include_related_spec_parts(":Hosts").is_err());

        // Empty class name
        assert!(parse_include_related_spec_parts("servers:").is_err());

        // Non-integer max_depth
        assert!(parse_include_related_spec_parts("servers:Hosts:bar").is_err());

        // Empty string
        assert!(parse_include_related_spec_parts("").is_err());
    }
}
