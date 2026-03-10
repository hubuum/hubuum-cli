use std::collections::HashMap;
use std::str::FromStr;

use hubuum_client::{
    ReportContentType, ReportLimits, ReportMissingDataPolicy, ReportOutputRequest, ReportRequest,
    ReportScope, ReportScopeKind, ReportTemplatePatch, ReportTemplatePost,
};

use crate::domain::{ReportOutput, ReportTemplateRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, FilterValueResolver, ListQuery, PagedResult,
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
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page =
            apply_query_paging(self.client.templates().find().filters(filters), query).page()?;
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

        let template = self.client.templates().create_raw(ReportTemplatePost {
            namespace_id: namespace.id(),
            name: input.name,
            description: input.description,
            content_type,
            template: input.template,
        })?;

        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);
        Ok(ReportTemplateRecord::new(&template, &namespacemap))
    }

    pub fn update_report_template(
        &self,
        input: UpdateReportTemplateInput,
    ) -> Result<ReportTemplateRecord, AppError> {
        let template = self.client.templates().select_by_name(&input.name)?;
        let namespace_id = match input.namespace {
            Some(namespace) => self.client.namespaces().select_by_name(&namespace)?.id(),
            None => template.resource().namespace_id,
        };

        let updated = self.client.templates().update_raw(
            template.id(),
            ReportTemplatePatch {
                namespace_id: Some(namespace_id),
                name: input.rename,
                description: input.description,
                template: input.template,
            },
        )?;

        let namespace = self.client.namespaces().select(updated.namespace_id)?;
        let namespacemap = HashMap::from([(namespace.id(), namespace.resource().clone())]);
        Ok(ReportTemplateRecord::new(&updated, &namespacemap))
    }

    pub fn delete_report_template(&self, name: &str) -> Result<(), AppError> {
        let template = self.client.templates().select_by_name(name)?;
        self.client.templates().delete(template.id())?;
        Ok(())
    }

    pub fn run_report(&self, input: RunReportInput) -> Result<ReportOutput, AppError> {
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

        let request = ReportRequest {
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
        };

        Ok(ReportOutput::from(self.client.reports().run(request)?))
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
