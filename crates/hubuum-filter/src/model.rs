use std::collections::HashSet;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::PipelineError;
use crate::selector::Selector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeStage {
    Grep(String),
    ValueSearch(String),
    KeySearch(String),
    Truthy(Option<Selector>),
    Reject(String),
    Head {
        count: usize,
        offset: usize,
    },
    Tail(usize),
    Count,
    SortLines {
        descending: bool,
    },
    Columns(Vec<ProjectTerm>),
    SortColumn {
        selector: Selector,
        descending: bool,
        cast: SortCast,
    },
    Group(Vec<GroupKey>),
    Aggregate(AggregateSpec),
    CollapseGroups,
    Unroll(Selector),
    Jq(String),
    Value(Selector),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTerm {
    selector: Selector,
    drop: bool,
}

impl ProjectTerm {
    pub fn keep(selector: impl Into<String>) -> Result<Self, PipelineError> {
        Ok(Self {
            selector: Selector::new(selector)?,
            drop: false,
        })
    }

    pub fn drop(selector: impl Into<String>) -> Result<Self, PipelineError> {
        Ok(Self {
            selector: Selector::new(selector)?,
            drop: true,
        })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn is_drop(&self) -> bool {
        self.drop
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupKey {
    selector: Selector,
    alias: OutputName,
}

impl GroupKey {
    pub fn new(
        selector: impl Into<String>,
        alias: impl Into<String>,
    ) -> Result<Self, PipelineError> {
        Ok(Self {
            selector: Selector::new(selector)?,
            alias: OutputName::new(alias)?,
        })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn alias(&self) -> &str {
        self.alias.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    function: AggregateFunction,
    alias: OutputName,
}

impl AggregateSpec {
    pub fn new(
        function: AggregateFunction,
        alias: impl Into<String>,
    ) -> Result<Self, PipelineError> {
        Ok(Self {
            function,
            alias: OutputName::new(alias)?,
        })
    }

    pub fn function(&self) -> &AggregateFunction {
        &self.function
    }

    pub fn alias(&self) -> &str {
        self.alias.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputName(String);

impl OutputName {
    pub fn new(value: impl Into<String>) -> Result<Self, PipelineError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(PipelineError::Pipe(
                "Pipe output name cannot be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for OutputName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateFunction {
    Count,
    Sum(Selector),
    Avg(Selector),
    Min(Selector),
    Max(Selector),
}

pub(crate) fn validate_projection_terms(terms: &[ProjectTerm]) -> Result<(), PipelineError> {
    ensure_unique_names(
        "P",
        "output column",
        terms
            .iter()
            .filter(|term| !term.is_drop())
            .map(|term| term.selector().as_str()),
    )
}

pub(crate) fn validate_group_keys(keys: &[GroupKey]) -> Result<(), PipelineError> {
    ensure_unique_names("G", "output name", keys.iter().map(GroupKey::alias))
}

fn ensure_unique_names<'a>(
    stage: &str,
    kind: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<(), PipelineError> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(PipelineError::Pipe(format!(
                "Pipe stage '{stage}' has duplicate {kind} '{name}'"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortCast {
    #[default]
    Auto,
    String,
    Number,
    Ip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputShape {
    Empty,
    Lines,
    Rows,
    Detail,
    Message,
    Values,
    Groups,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputEnvelope {
    pub shape: OutputShape,
    pub value: Value,
    pub columns: Vec<String>,
}

impl OutputEnvelope {
    pub fn empty() -> Self {
        Self {
            shape: OutputShape::Empty,
            value: Value::Array(Vec::new()),
            columns: Vec::new(),
        }
    }

    pub fn lines(lines: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Lines,
            value: Value::Array(lines.into_iter().map(Value::String).collect()),
            columns: Vec::new(),
        }
    }

    pub fn rows(rows: Vec<Value>, columns: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Rows,
            value: Value::Array(rows),
            columns,
        }
    }

    pub fn detail(value: Value, columns: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Detail,
            value,
            columns,
        }
    }

    pub fn message(value: Value) -> Self {
        Self {
            shape: OutputShape::Message,
            value,
            columns: Vec::new(),
        }
    }

    pub fn values(values: Vec<Value>) -> Self {
        Self {
            shape: OutputShape::Values,
            value: Value::Array(values),
            columns: vec!["value".to_string()],
        }
    }

    pub fn groups(groups: Vec<Value>, columns: Vec<String>) -> Self {
        Self {
            shape: OutputShape::Groups,
            value: Value::Array(groups),
            columns,
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.value {
            Value::Array(items) => items.is_empty(),
            Value::Null => true,
            _ => false,
        }
    }
}
