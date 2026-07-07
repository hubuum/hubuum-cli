use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeStage {
    Grep(String),
    ValueSearch(String),
    KeySearch(String),
    Truthy(Option<String>),
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
        column: String,
        descending: bool,
        cast: SortCast,
    },
    Group(Vec<GroupKey>),
    Aggregate(AggregateSpec),
    CollapseGroups,
    Unroll(String),
    Jq(String),
    Value(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTerm {
    pub selector: String,
    pub drop: bool,
}

impl ProjectTerm {
    pub fn keep(selector: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            drop: false,
        }
    }

    pub fn drop(selector: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            drop: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupKey {
    pub selector: String,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    pub function: AggregateFunction,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateFunction {
    Count,
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
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
