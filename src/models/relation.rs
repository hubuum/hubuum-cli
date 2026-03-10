use crate::{
    domain::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord},
    errors::AppError,
    formatting::OutputFormatter,
};

pub enum Relation {
    Class(ResolvedClassRelationRecord),
    Object(ResolvedObjectRelationRecord),
}

impl Relation {
    pub fn format_json_noreturn(&self) -> Result<(), AppError> {
        match self {
            Relation::Class(r) => r.format_json_noreturn(),
            Relation::Object(r) => r.format_json_noreturn(),
        }
    }

    pub fn format_noreturn(&self) -> Result<(), AppError> {
        match self {
            Relation::Class(r) => r.format_noreturn(),
            Relation::Object(r) => r.format_noreturn(),
        }
    }
}

impl From<ResolvedClassRelationRecord> for Relation {
    fn from(r: ResolvedClassRelationRecord) -> Self {
        Relation::Class(r)
    }
}

impl From<ResolvedObjectRelationRecord> for Relation {
    fn from(r: ResolvedObjectRelationRecord) -> Self {
        Relation::Object(r)
    }
}
