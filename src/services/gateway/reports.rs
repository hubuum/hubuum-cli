use std::collections::HashMap;
use std::str::FromStr;

use hubuum_client::{
    ReportContentType, ReportInclude, ReportIncludeRelatedObject, ReportLimits,
    ReportMissingDataPolicy, ReportOutputRequest, ReportRelationContext, ReportRequest,
    ReportScope, ReportScopeKind, ReportTemplatePatch, ReportTemplatePost,
};

use crate::domain::{ReportTemplateRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
    SortFieldSpec,
};

use super::{shared::find_entities_by_ids, HubuumGateway};

#[derive(Debug, Clone)]
pub struct CreateReportTemplateInput {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub content_type: String,
    pub template: String,
}

#[derive(Debug, Clone)]
pub struct UpdateReportTemplateInput {
    pub name: String,
    pub rename: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub template: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunReportInput {
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

/// Parse a single --include-related spec into (key, ReportIncludeRelatedObject).
///
/// Format: `<key>:<class_id>[:<max_depth>]`
/// - `<key>`: arbitrary string (map key for the related object set)
/// - `<class_id>`: integer class ID
/// - `<max_depth>`: optional integer for max traversal depth
///
/// Examples:
/// - `servers:42` → key="servers", class_id=42, max_depth=None
/// - `servers:42:3` → key="servers", class_id=42, max_depth=Some(3)
fn parse_include_related_spec(spec: &str) -> Result<(String, ReportIncludeRelatedObject), AppError> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() < 2 {
        return Err(AppError::ParseError(format!(
            "Invalid --include-related spec '{}': expected '<key>:<class_id>[:<max_depth>]'",
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

    let class_id = parts[1].parse::<i32>().map_err(|_| {
        AppError::ParseError(format!(
            "Invalid --include-related spec '{}': class_id must be an integer",
            spec
        ))
    })?;

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

    Ok((
        key,
        ReportIncludeRelatedObject {
            class_id,
            class_relation_id: None,
            direction: None,
            limit: None,
            max_depth,
            sort: None,
        },
    ))
}

impl HubuumGateway {
    pub fn list_report_template_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .templates()
            .find()
            .execute()?
            .into_iter()
            .map(|template| template.name)
            .collect())
    }

    pub fn list_report_templates(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ReportTemplateRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, REPORT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, REPORT_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = apply_query_paging(
            self.client.templates().find().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        if page.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                next_cursor: page.next_cursor,
                limit: query.limit,
                returned_count: 0,
            });
        }

        let namespacemap =
            find_entities_by_ids(&self.client.namespaces(), page.items.iter(), |template| {
                template.namespace_id
            })?;

        Ok(PagedResult::from_page(page, query.limit, |template| {
            ReportTemplateRecord::new(&template, &namespacemap)
        }))
    }

    pub fn report_template(&self, name: &str) -> Result<ReportTemplateRecord, AppError> {
        let template = self.client.templates().select_by_name(name)?;
        let namespace = self
            .client
            .namespaces()
            .select(template.resource().namespace_id)?;
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);

        Ok(ReportTemplateRecord::new(
            template.resource(),
            &namespacemap,
        ))
    }

    pub fn create_report_template(
        &self,
        input: CreateReportTemplateInput,
    ) -> Result<ReportTemplateRecord, AppError> {
        let namespace = self.client.namespaces().select_by_name(&input.namespace)?;
        let content_type = ReportContentType::from_str(&input.content_type).map_err(|_| {
            AppError::ParseError(format!("Invalid content type: {}", input.content_type))
        })?;

        let template = self
            .client
            .templates()
            .create()
            .params(ReportTemplatePost {
                namespace_id: namespace.id(),
                name: input.name,
                description: input.description,
                content_type,
                template: input.template,
            })
            .send()?;

        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);
        Ok(ReportTemplateRecord::new(&template, &namespacemap))
    }

    pub fn update_report_template(
        &self,
        input: UpdateReportTemplateInput,
    ) -> Result<ReportTemplateRecord, AppError> {
        let template = self.client.templates().select_by_name(&input.name)?;
        let namespace_id = match input.namespace {
            Some(namespace) => Some(self.client.namespaces().select_by_name(&namespace)?.id()),
            None => None,
        };

        let updated = self
            .client
            .templates()
            .update(template.id())
            .params(ReportTemplatePatch {
                namespace_id,
                name: input.rename,
                description: input.description,
                template: input.template,
            })
            .send()?;

        let namespace = self.client.namespaces().select(updated.namespace_id)?;
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);
        Ok(ReportTemplateRecord::new(&updated, &namespacemap))
    }

    pub fn delete_report_template(&self, name: &str) -> Result<(), AppError> {
        let template = self.client.templates().select_by_name(name)?;
        self.client.templates().delete(template.id())?;
        Ok(())
    }

    fn build_report_request(&self, input: RunReportInput) -> Result<ReportRequest, AppError> {
        let scope_kind = ReportScopeKind::from_str(&input.scope_kind).map_err(|_| {
            AppError::ParseError(format!("Invalid report scope: {}", input.scope_kind))
        })?;

        let class_id = match &input.class_name {
            Some(name) => Some(self.client.classes().select_by_name(name)?.id()),
            None => None,
        };

        let object_id = match (&input.class_name, &input.object_name) {
            (Some(class_name), Some(object_name)) => {
                let class = self.client.classes().select_by_name(class_name)?;
                Some(class.object_by_name(object_name)?.id())
            }
            (None, Some(_)) => {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            _ => None,
        };

        validate_report_scope(&scope_kind, class_id, object_id)?;

        let template_id = match input.template {
            Some(template_name) => {
                Some(self.client.templates().select_by_name(&template_name)?.id())
            }
            None => None,
        };

        let missing_data_policy = match input.missing_data_policy {
            Some(policy) => Some(ReportMissingDataPolicy::from_str(&policy).map_err(|_| {
                AppError::ParseError(format!("Invalid missing data policy: {policy}"))
            })?),
            None => None,
        };

        let relation_context = input
            .relation_depth
            .map(|depth| ReportRelationContext { depth: Some(depth) });

        let include = if !input.include_related.is_empty() {
            let mut related_objects = HashMap::new();
            for spec in &input.include_related {
                let (key, obj) = parse_include_related_spec(spec)?;
                related_objects.insert(key, obj);
            }
            Some(ReportInclude {
                related_objects: Some(related_objects),
            })
        } else {
            None
        };

        Ok(ReportRequest {
            limits: if input.max_items.is_some() || input.max_output_bytes.is_some() {
                Some(ReportLimits {
                    max_items: input.max_items,
                    max_output_bytes: input.max_output_bytes,
                })
            } else {
                None
            },
            missing_data_policy,
            output: template_id.map(|template_id| ReportOutputRequest {
                template_id: Some(template_id),
            }),
            query: input.query,
            scope: ReportScope {
                class_id,
                kind: scope_kind,
                object_id,
            },
            include,
            relation_context,
        })
    }

    pub fn submit_report(&self, input: RunReportInput) -> Result<TaskRecord, AppError> {
        let request = self.build_report_request(input)?;
        Ok(TaskRecord(self.client.reports().submit(request).send()?))
    }
}

pub(crate) const REPORT_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "namespace",
        "namespace_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::NamespaceNameToId),
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

pub(crate) const REPORT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("namespace", "namespace_id"),
    SortFieldSpec::new("content_type", "content_type"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

fn validate_report_scope(
    scope_kind: &ReportScopeKind,
    class_id: Option<i32>,
    object_id: Option<i32>,
) -> Result<(), AppError> {
    match scope_kind {
        ReportScopeKind::Namespaces => {
            if class_id.is_some() || object_id.is_some() {
                return Err(AppError::ParseError(
                    "namespace reports do not take class or object".to_string(),
                ));
            }
        }
        ReportScopeKind::Classes
        | ReportScopeKind::ObjectsInClass
        | ReportScopeKind::ClassRelations
        | ReportScopeKind::ObjectRelations => {
            if class_id.is_none() {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            if object_id.is_some() {
                return Err(AppError::ParseError(
                    "this report scope does not take an object".to_string(),
                ));
            }
        }
        ReportScopeKind::RelatedObjects => {
            if class_id.is_none() {
                return Err(AppError::MissingOptions(vec!["class".to_string()]));
            }
            if object_id.is_none() {
                return Err(AppError::MissingOptions(vec!["object".to_string()]));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_include_related_spec_valid() {
        // Test basic format: key:class_id
        let (key, obj) = parse_include_related_spec("servers:42").unwrap();
        assert_eq!(key, "servers");
        assert_eq!(obj.class_id, 42);
        assert_eq!(obj.max_depth, None);
        assert_eq!(obj.class_relation_id, None);
        assert_eq!(obj.direction, None);
        assert_eq!(obj.limit, None);
        assert_eq!(obj.sort, None);

        // Test with max_depth: key:class_id:max_depth
        let (key, obj) = parse_include_related_spec("servers:42:3").unwrap();
        assert_eq!(key, "servers");
        assert_eq!(obj.class_id, 42);
        assert_eq!(obj.max_depth, Some(3));

        // Test with complex key
        let (key, obj) = parse_include_related_spec("my_servers:100:5").unwrap();
        assert_eq!(key, "my_servers");
        assert_eq!(obj.class_id, 100);
        assert_eq!(obj.max_depth, Some(5));
    }

    #[test]
    fn test_parse_include_related_spec_invalid() {
        // Missing class_id
        assert!(parse_include_related_spec("servers").is_err());

        // Empty key
        assert!(parse_include_related_spec(":42").is_err());

        // Non-integer class_id
        assert!(parse_include_related_spec("servers:foo").is_err());

        // Non-integer max_depth
        assert!(parse_include_related_spec("servers:42:bar").is_err());

        // Empty string
        assert!(parse_include_related_spec("").is_err());
    }
}
