use crate::diagnostics::client_call;

use hubuum_client::{
    client::{sync::CursorRequest, sync::QueryOp},
    ApiResource,
};
use serde::de::DeserializeOwned;

use crate::errors::AppError;

use super::{ListQuery, PagedResult, ValidatedSortClause};

fn apply_query_paging<T>(
    query: QueryOp<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> QueryOp<T>
where
    T: ApiResource,
{
    let query = query.include_total(list_query.include_total);
    let query = if sorts.is_empty() {
        query
    } else {
        query.sort_by_fields(
            sorts
                .iter()
                .map(|clause| (clause.spec().backend_field(), clause.direction().to_api()))
                .collect::<Vec<_>>(),
        )
    };

    let query = if let Some(limit) = list_query.limit {
        query.limit(limit.effective())
    } else {
        query
    };

    if let Some(cursor) = &list_query.cursor {
        query.cursor(cursor)
    } else {
        query
    }
}

pub(crate) fn fetch_query_results<T>(
    query: QueryOp<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> Result<PagedResult<T::GetOutput>, AppError>
where
    T: ApiResource,
{
    let query = apply_query_paging(query, list_query, sorts);
    if list_query.fetches_all() {
        PagedResult::from_pages(query.pages())
    } else {
        Ok(PagedResult::from_page(
            client_call("QueryOp.page", || query.page())?,
            |item| item,
        ))
    }
}

fn apply_cursor_request_paging<T>(
    request: CursorRequest<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> CursorRequest<T> {
    let request = request.include_total(list_query.include_total);
    let request = if sorts.is_empty() {
        request
    } else {
        request.sort_by_fields(
            sorts
                .iter()
                .map(|clause| (clause.spec().backend_field(), clause.direction().to_api()))
                .collect::<Vec<_>>(),
        )
    };

    let request = if let Some(limit) = list_query.limit {
        request.limit(limit.effective())
    } else {
        request
    };

    if let Some(cursor) = &list_query.cursor {
        request.cursor(cursor)
    } else {
        request
    }
}

pub(crate) fn fetch_cursor_results<T>(
    request: CursorRequest<T>,
    list_query: &ListQuery,
    sorts: &[ValidatedSortClause],
) -> Result<PagedResult<T>, AppError>
where
    T: DeserializeOwned,
{
    let request = apply_cursor_request_paging(request, list_query, sorts);
    if list_query.fetches_all() {
        PagedResult::from_pages(request.pages())
    } else {
        Ok(PagedResult::from_page(
            client_call("CursorRequest.page", || request.page())?,
            |item| item,
        ))
    }
}
