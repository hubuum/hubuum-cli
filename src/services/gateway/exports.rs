use std::collections::HashMap;
use std::str::FromStr;

use hubuum_client::{
    ClassId, ExportContentType, ExportInclude, ExportIncludeRelatedObject, ExportLimits,
    ExportMissingDataPolicy, ExportRelationContext, ExportRequest, ExportScope, ExportScopeKind,
    ExportTemplateKind, ExportTemplatePatch, ExportTemplateRunRequest, ObjectId,
};

use crate::domain::{ExportTemplateRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
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
pub struct RunExportInput {
    pub template: Option<String>,
    pub scope_kind: String,
    pub class_name: Option<String>,
    pub object_name: Option<String>,
    pub query: Option<String>,
    pub missing_data_policy: Option<String>,
    pub max_items: Option<u64>,
    pub max_output_bytes: Option<u64>,
    pub relation_depth: Option<i32>,
    pub include_related: Vec<String>,
}

/// Parse a single --include-related spec into (key, ExportIncludeRelatedObject).
///
/// Format: `<key>:<class_name>[:<max_depth>]`
/// - `<key>`: arbitrary string (map key for the related object set)
/// - `<class_name>`: class name
/// - `<max_depth>`: optional integer for max traversal depth
///
/// Examples:
/// - `servers:Hosts` → key="servers", class_id=<Hosts id>, max_depth=None
/// - `servers:Hosts:3` → key="servers", class_id=<Hosts id>, max_depth=Some(3)
fn parse_include_related_spec(
    gateway: &HubuumGateway,
    spec: &str,
) -> Result<(String, ExportIncludeRelatedObject), AppError> {
    let (key, class_name, max_depth) = parse_include_related_spec_parts(spec)?;
    let class_id = gateway.class_handle_by_name(&class_name)?.id();

    Ok((
        key,
        ExportIncludeRelatedObject {
            class_id,
            class_relation_id: None,
            direction: None,
            limit: None,
            max_depth,
            sort: None,
        },
    ))
}

fn parse_include_related_spec_parts(spec: &str) -> Result<(String, String, Option<i32>), AppError> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() < 2 {
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
        Ok(self
            .client
            .export_templates()
            .query()
            .list()?
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
        let page = apply_query_paging(
            self.client.export_templates().query().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                returned_count: 0,
                total_count: page.total_count,
            });
        }

        let collectionmap =
            find_entities_by_ids(&self.client.collections(), page.items.iter(), |template| {
                template.collection_id
            })?;

        Ok(PagedResult::from_page(page, |template| {
            ExportTemplateRecord::new(&template, &collectionmap)
        }))
    }

    pub fn export_template(&self, name: &str) -> Result<ExportTemplateRecord, AppError> {
        let template = self.client.export_templates().get_by_name(name)?;
        let collection = self
            .client
            .collections()
            .get(template.resource().collection_id)?;
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);

        Ok(ExportTemplateRecord::new(
            template.resource(),
            &collectionmap,
        ))
    }

    pub fn create_export_template(
        &self,
        input: CreateExportTemplateInput,
    ) -> Result<ExportTemplateRecord, AppError> {
        let collection = self.client.collections().get_by_name(&input.collection)?;
        let content_type = ExportContentType::from_str(&input.content_type).map_err(|_| {
            AppError::ParseError(format!("Invalid content type: {}", input.content_type))
        })?;

        let template = self
            .client
            .export_templates()
            .create_checked()
            .collection_id(collection.id())
            .name(input.name)
            .description(input.description)
            .content_type(content_type)
            .template(input.template)
            .kind(ExportTemplateKind::Export)
            .send()?;

        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);
        Ok(ExportTemplateRecord::new(&template, &collectionmap))
    }

    pub fn update_export_template(
        &self,
        input: UpdateExportTemplateInput,
    ) -> Result<ExportTemplateRecord, AppError> {
        let template = self.client.export_templates().get_by_name(&input.name)?;
        let collection_id = match input.collection {
            Some(collection) => Some(self.client.collections().get_by_name(&collection)?.id()),
            None => None,
        };

        let updated = self
            .client
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
            .send()?;

        let collection = self.client.collections().get(updated.collection_id)?;
        let collectionmap =
            HashMap::from([(collection.id().into(), collection.resource().clone())]);
        Ok(ExportTemplateRecord::new(&updated, &collectionmap))
    }

    pub fn delete_export_template(&self, name: &str) -> Result<(), AppError> {
        let template = self.client.export_templates().get_by_name(name)?;
        self.client.export_templates().delete(template.id())?;
        Ok(())
    }

    fn build_export_request(&self, input: &RunExportInput) -> Result<ExportRequest, AppError> {
        let scope_kind = ExportScopeKind::from_str(&input.scope_kind).map_err(|_| {
            AppError::ParseError(format!("Invalid export scope: {}", input.scope_kind))
        })?;

        let class_id = match &input.class_name {
            Some(name) => Some(self.client.classes().get_by_name(name)?.id()),
            None => None,
        };

        let object_id = match (&input.class_name, &input.object_name) {
            (Some(class_name), Some(object_name)) => {
                let class = self.client.classes().get_by_name(class_name)?;
                Some(class.object_by_name(object_name)?.id())
            }
            (None, Some(_)) => {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            _ => None,
        };

        validate_export_scope(&scope_kind, class_id, object_id)?;

        let missing_data_policy = match &input.missing_data_policy {
            Some(policy) => Some(ExportMissingDataPolicy::from_str(policy).map_err(|_| {
                AppError::ParseError(format!("Invalid missing data policy: {policy}"))
            })?),
            None => None,
        };

        let relation_context = input
            .relation_depth
            .map(|depth| ExportRelationContext { depth: Some(depth) });

        let include = if !input.include_related.is_empty() {
            let mut related_objects = HashMap::new();
            for spec in &input.include_related {
                let (key, obj) = parse_include_related_spec(self, spec)?;
                related_objects.insert(key, obj);
            }
            Some(ExportInclude {
                related_objects: Some(related_objects),
            })
        } else {
            None
        };

        Ok(ExportRequest {
            limits: if input.max_items.is_some() || input.max_output_bytes.is_some() {
                Some(ExportLimits {
                    max_items: input.max_items,
                    max_output_bytes: input.max_output_bytes,
                })
            } else {
                None
            },
            missing_data_policy,
            query: input.query.clone(),
            scope: ExportScope {
                class_id,
                kind: scope_kind,
                object_id,
            },
            include,
            relation_context,
        })
    }

    pub fn submit_export(&self, input: RunExportInput) -> Result<TaskRecord, AppError> {
        if let Some(template_name) = &input.template {
            let template = self.client.export_templates().get_by_name(template_name)?;
            let class = match &input.class_name {
                Some(class_name) => Some(self.client.classes().get_by_name(class_name)?),
                None => None,
            };
            let object_id = match (&class, &input.object_name) {
                (Some(class), Some(object_name)) => Some(class.object_by_name(object_name)?.id()),
                (None, Some(_)) => return Err(AppError::MissingOptions(vec!["class".to_string()])),
                _ => None,
            };
            let missing_data_policy = match &input.missing_data_policy {
                Some(policy) => Some(ExportMissingDataPolicy::from_str(policy).map_err(|_| {
                    AppError::ParseError(format!("Invalid missing data policy: {policy}"))
                })?),
                None => None,
            };
            let limits = if input.max_items.is_some() || input.max_output_bytes.is_some() {
                Some(ExportLimits {
                    max_items: input.max_items,
                    max_output_bytes: input.max_output_bytes,
                })
            } else {
                None
            };
            let request = ExportTemplateRunRequest {
                query: input.query,
                object_id,
                missing_data_policy,
                limits,
            };
            return Ok(TaskRecord(
                self.client
                    .export_templates()
                    .submit_export(template.id(), request)
                    .send()?,
            ));
        }

        let request = self.build_export_request(&input)?;
        Ok(TaskRecord(self.client.exports().submit(request).send()?))
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

fn validate_export_scope(
    scope_kind: &ExportScopeKind,
    class_id: Option<ClassId>,
    object_id: Option<ObjectId>,
) -> Result<(), AppError> {
    match scope_kind {
        ExportScopeKind::Collections => {
            if class_id.is_some() || object_id.is_some() {
                return Err(AppError::ParseError(
                    "collection exports do not take class or object".to_string(),
                ));
            }
        }
        ExportScopeKind::Classes
        | ExportScopeKind::ObjectsInClass
        | ExportScopeKind::ClassRelations
        | ExportScopeKind::ObjectRelations => {
            if class_id.is_none() {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            if object_id.is_some() {
                return Err(AppError::ParseError(
                    "this export scope does not take an object".to_string(),
                ));
            }
        }
        ExportScopeKind::RelatedObjects => {
            if class_id.is_none() {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            if object_id.is_none() {
                return Err(AppError::MissingOptions(vec!["object".to_string()]));
            }
        }
        _ => {
            return Err(AppError::ParseError(format!(
                "unsupported export scope: {scope_kind}"
            )));
        }
    }

    Ok(())
}

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
