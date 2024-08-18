use chrono::Utc;
use log::trace;

pub fn with_timing<F, R>(label: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    let start = Utc::now();
    let result = f();
    let elapsed = Utc::now().signed_duration_since(start);
    trace!("{}: {}ms", label, elapsed.num_milliseconds());
    result
}
