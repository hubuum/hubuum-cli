use hubuum_client::{SearchHydrationRequest, StructuredClassSelector};

use super::{HubuumGateway, ResourceScope};
use crate::diagnostics::client_call;
use crate::domain::JsonRecord;
use crate::errors::AppError;

pub struct HydrationOptions {
    target: StructuredClassSelector,
    depth: u8,
    limit: u8,
}

impl HydrationOptions {
    pub fn new(target: StructuredClassSelector) -> Self {
        Self {
            target,
            depth: 1,
            limit: 20,
        }
    }
    pub fn depth(mut self, depth: u8) -> Result<Self, AppError> {
        if !(1..=10).contains(&depth) {
            return Err(AppError::InvalidOption(
                "--max-depth must be between 1 and 10".into(),
            ));
        }
        self.depth = depth;
        Ok(self)
    }
    pub fn path_limit(mut self, limit: u8) -> Result<Self, AppError> {
        if !(1..=50).contains(&limit) {
            return Err(AppError::InvalidOption(
                "--path-limit must be between 1 and 50".into(),
            ));
        }
        self.limit = limit;
        Ok(self)
    }
}

impl HubuumGateway {
    pub fn hydrate_object(
        &self,
        root: ResourceScope,
        options: HydrationOptions,
    ) -> Result<JsonRecord, AppError> {
        let (class, object) = self.resolve_resource_scope(root.require_object()?)?;
        let request = SearchHydrationRequest::new(
            class,
            object.expect("required object"),
            options.target,
            options.depth,
            options.limit,
        )?;
        Ok(JsonRecord::from_serializable(client_call(
            "client.hydrate_search",
            || self.client().hydrate_search(request),
        )?)?)
    }
}
