use crate::diagnostics::{client_call, stage_sync};
use crate::domain::{ClassFieldSample, ObjectFieldValues, ResourceName};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, fetch_query_results, ListQuery, PageSelection, ServerPageSize,
};

use super::HubuumGateway;

impl HubuumGateway {
    pub(crate) fn sample_class_fields(
        &self,
        class_name: &ResourceName,
        limit: ServerPageSize,
    ) -> Result<ClassFieldSample, AppError> {
        let class = self.class_handle_by_name(class_name.as_str())?;
        let shared = client_call("computed_fields.list", || {
            self.client().computed_fields(class.id()).list()
        })?;
        let personal = fetch_cursor_results(
            self.client()
                .personal_computed_fields()
                .for_class(class.id()),
            &ListQuery::default().page_selection(PageSelection::All),
            &[],
        )?;
        let shared_keys = shared
            .definitions
            .into_iter()
            .filter(|field| field.enabled)
            .map(|field| field.key)
            .collect::<Vec<_>>();
        let personal_keys = personal
            .items
            .into_iter()
            .filter(|field| field.enabled)
            .map(|field| field.key)
            .collect::<Vec<_>>();
        let query = ListQuery {
            limit: Some(limit),
            ..ListQuery::default()
        };
        let objects = if shared_keys.is_empty() && personal_keys.is_empty() {
            let page = fetch_query_results(self.client().objects(class.id()).query(), &query, &[])?;
            stage_sync("prepare_field_sample", || {
                page.items
                    .into_iter()
                    .map(ObjectFieldValues::from)
                    .collect()
            })
        } else {
            let page =
                fetch_cursor_results(self.client().computed_objects(class.id()), &query, &[])?;
            stage_sync("prepare_field_sample", || {
                page.items
                    .into_iter()
                    .map(ObjectFieldValues::from)
                    .collect()
            })
        };
        Ok(ClassFieldSample::new(shared_keys, personal_keys, objects))
    }
}
