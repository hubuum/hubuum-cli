use std::fmt;

use hubuum_client::{HistoryPage, Page};
use serde::Serialize;
use serde_json::{json, to_value, Value};

use crate::errors::AppError;
use crate::formatting::TableRenderable;
use crate::list_query::PagedResult;

/// Retains the complete snapshot for JSON while giving tables history-specific columns.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct HistoryRecord(Value);

impl HistoryRecord {
    pub(crate) fn new(value: impl Serialize) -> Result<Self, AppError> {
        Ok(Self(to_value(value)?))
    }

    fn field(&self, field: &str) -> String {
        match &self.0[field] {
            Value::Null => "-".into(),
            Value::String(value) => value.clone(),
            value => value.to_string(),
        }
    }
}

impl fmt::Debug for HistoryRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryRecord").finish_non_exhaustive()
    }
}

impl TableRenderable for HistoryRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "history_id",
            "id",
            "name",
            "revision",
            "operation",
            "valid_from",
            "valid_to",
        ]
    }

    fn row(&self) -> Vec<String> {
        Self::headers()
            .iter()
            .map(|field| {
                if *field == "operation" && self.0.get("operation").is_none() {
                    self.field("op")
                } else {
                    self.field(field)
                }
            })
            .collect()
    }
}

/// Historical envelopes must survive pagination and pipelines without losing context.
#[derive(Clone, Serialize)]
pub(crate) struct HistoryPageRecord {
    #[serde(flatten)]
    body: Value,
    next_cursor: Option<String>,
    total_count: Option<u64>,
    page_limit: Option<usize>,
    returned_count: usize,
    #[serde(skip)]
    row_key: &'static str,
}

impl HistoryPageRecord {
    pub(crate) fn records<T: Serialize>(page: Page<T>) -> Result<Self, AppError> {
        let returned_count = page.items.len();
        Ok(Self {
            body: json!({"items": page.items}),
            next_cursor: page.next_cursor,
            total_count: page.total_count,
            page_limit: page.page_limit,
            returned_count,
            row_key: "items",
        })
    }

    pub(crate) fn envelope<T: Serialize>(
        page: HistoryPage<T>,
        row_key: &'static str,
    ) -> Result<Self, AppError> {
        let body = to_value(page.data)?;
        let returned_count = body[row_key]
            .as_array()
            .ok_or_else(|| AppError::ParseError("Historical response is missing its rows".into()))?
            .len();
        Ok(Self {
            body,
            next_cursor: page.next_cursor,
            total_count: page.total_count,
            page_limit: page.page_limit,
            returned_count,
            row_key,
        })
    }

    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
    pub(crate) fn len(&self) -> usize {
        self.returned_count
    }
    pub(crate) fn context(&self) -> Value {
        Value::Object(
            self.body
                .as_object()
                .expect("historical envelope")
                .iter()
                .filter(|(key, _)| key.as_str() != self.row_key)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }

    pub(crate) fn relation_rows(&self) -> Option<Vec<HistoricalRelationRecord>> {
        (self.row_key == "relations").then(|| {
            self.body[self.row_key]
                .as_array()
                .expect("validated rows")
                .iter()
                .cloned()
                .map(|value| HistoricalRelationRecord(HistoryRecord(value)))
                .collect()
        })
    }

    pub(crate) fn append(&mut self, mut next: Self) -> Result<(), AppError> {
        if self.row_key != next.row_key
            || self.context() != next.context()
            || self.total_count != next.total_count
            || self.page_limit != next.page_limit
        {
            return Err(AppError::InvalidOption(
                "Historical context changed between pages; restart the query".into(),
            ));
        }
        self.body[self.row_key]
            .as_array_mut()
            .expect("validated rows")
            .append(
                next.body[next.row_key]
                    .as_array_mut()
                    .expect("validated rows"),
            );
        self.returned_count += next.returned_count;
        self.next_cursor = next.next_cursor;
        Ok(())
    }

    pub(crate) fn rows(&self) -> Result<PagedResult<HistoryRecord>, AppError> {
        Ok(PagedResult {
            items: self.body[self.row_key]
                .as_array()
                .expect("validated rows")
                .iter()
                .map(HistoryRecord::new)
                .collect::<Result<_, _>>()?,
            next_cursor: self.next_cursor.clone(),
            returned_count: self.returned_count,
            total_count: self.total_count,
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct HistoricalRelationRecord(HistoryRecord);

impl TableRenderable for HistoricalRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "history_id",
            "revision",
            "operation",
            "from",
            "to",
            "forward_label",
            "reverse_label",
            "context_at",
        ]
    }
    fn row(&self) -> Vec<String> {
        Self::headers()
            .iter()
            .map(|field| match *field {
                "operation" => self.0.field("op"),
                "from" | "to" => {
                    let endpoint = &self.0 .0[field];
                    if endpoint["status"] == "available" {
                        format!(
                            "{} (#{})",
                            endpoint["snapshot"]["name"].as_str().unwrap_or("-"),
                            endpoint["snapshot"]["id"]
                        )
                    } else {
                        format!(
                            "unavailable: {}",
                            endpoint["reason"].as_str().unwrap_or("history_unavailable")
                        )
                    }
                }
                "forward_label" | "reverse_label" => self.0 .0["relation"][field]
                    .as_str()
                    .unwrap_or("-")
                    .to_owned(),
                field => self.0.field(field),
            })
            .collect()
    }
}

impl fmt::Debug for HistoryPageRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryPageRecord")
            .field("returned_count", &self.returned_count)
            .finish_non_exhaustive()
    }
}
