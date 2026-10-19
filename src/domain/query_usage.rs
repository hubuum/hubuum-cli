use std::str::FromStr;

use hubuum_client::{
    QueryUsageAnalysisResponse, QueryUsageDeclarationId, QueryUsageResponse, ResourceRevision,
};
use serde::Serialize;

use crate::errors::AppError;

#[derive(Debug, Clone, Copy)]
pub struct QueryUsageId(QueryUsageDeclarationId);

impl QueryUsageId {
    pub const fn get(self) -> QueryUsageDeclarationId {
        self.0
    }
}

impl FromStr for QueryUsageId {
    type Err = AppError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.parse::<i32>().map_err(|_| {
            AppError::InvalidOption("Declaration ID must be a positive integer".into())
        })?;
        Ok(Self(QueryUsageDeclarationId::new(value)?))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExpectedQueryUsageRevision(ResourceRevision);

impl ExpectedQueryUsageRevision {
    pub const fn get(self) -> ResourceRevision {
        self.0
    }
}

impl FromStr for ExpectedQueryUsageRevision {
    type Err = AppError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.parse::<i64>().map_err(|_| {
            AppError::InvalidOption("Expected revision must be a positive integer".into())
        })?;
        Ok(Self(ResourceRevision::new(value)?))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QueryUsageOutput {
    List(Vec<QueryUsageResponse>),
    Declaration(Box<QueryUsageResponse>),
    Analysis(Box<QueryUsageAnalysisResponse>),
    Deleted { deleted: QueryUsageDeclarationId },
}
