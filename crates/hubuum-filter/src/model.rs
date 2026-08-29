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
    alias: String,
}

impl GroupKey {
    pub fn new(
        selector: impl Into<String>,
        alias: impl Into<String>,
    ) -> Result<Self, PipelineError> {
        Ok(Self {
            selector: Selector::new(selector)?,
            alias: alias.into(),
        })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    pub function: AggregateFunction,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateFunction {
    Count,
    Sum(Selector),
    Avg(Selector),
    Min(Selector),
    Max(Selector),
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
