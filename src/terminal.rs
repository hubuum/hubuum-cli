use std::env::var;

use crossterm::terminal::size;

pub(crate) fn terminal_width() -> Option<usize> {
    size()
        .ok()
        .map(|(width, _)| usize::from(width))
        .filter(|width| *width > 0)
        .or_else(columns_env_width)
}

fn columns_env_width() -> Option<usize> {
    var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
}
