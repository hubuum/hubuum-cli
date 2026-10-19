use std::fmt::{self, Write};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{debug_span, dispatcher, field, Event, Instrument, Span, Subscriber};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const TARGET: &str = "hubuum_cli::diagnostics";
const CLIENT_TARGET: &str = "hubuum_client::diagnostics";
static DEBUG_DEFAULT: AtomicBool = AtomicBool::new(false);

tokio::task_local! {
    static TRACE: CommandTrace;
}

#[derive(Clone)]
struct CommandTrace(Arc<Mutex<CommandReport>>);

struct CommandReport {
    enabled: bool,
    duration: Option<Duration>,
    spans: Vec<TimedSpan>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Stage,
    ClientCall,
    ClientLibrary,
    Page,
}

struct TimedSpan {
    kind: SpanKind,
    name: String,
    depth: usize,
    duration: Option<Duration>,
    succeeded: Option<bool>,
    empty: bool,
    client: Option<ClientFields>,
}

struct CapturedSpan {
    trace: CommandTrace,
    started: Instant,
    index: Option<usize>,
    depth: usize,
}

#[derive(Default)]
struct SpanFields {
    name: Option<String>,
    succeeded: Option<bool>,
    empty: bool,
    finished: bool,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if matches!(field.name(), "operation" | "stage") {
            self.name = Some(value.to_owned());
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        match field.name() {
            "succeeded" => self.succeeded = Some(value),
            "empty" => self.empty = value,
            "finished" => self.finished = value,
            _ => {}
        }
    }

    // Never collect arbitrary arguments, return values, or error messages.
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}
}

#[derive(Default)]
struct ClientFields {
    version: Option<u64>,
    method: Option<String>,
    route: Option<String>,
    transport: Option<&'static str>,
    attempt: Option<u64>,
    status: Option<u64>,
    outcome: Option<&'static str>,
    duration: Option<Duration>,
    page: Option<u64>,
    items: Option<u64>,
}

impl Visit for ClientFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "http.request.method" => self.method = Some(value.to_owned()),
            "http.route" => self.route = Some(value.to_owned()),
            "transport" => {
                self.transport = match value {
                    "reqwest" => Some("reqwest"),
                    "custom" => Some("custom"),
                    _ => None,
                }
            }
            "outcome" => {
                self.outcome = match value {
                    "ok" => Some("ok"),
                    "error" => Some("failed"),
                    "cancelled" => Some("cancelled"),
                    _ => None,
                }
            }
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "diagnostics.version" => self.version = Some(value),
            "attempt" => self.attempt = Some(value),
            "http.response.status_code" if (100..=599).contains(&value) => {
                self.status = Some(value)
            }
            "page" => self.page = Some(value),
            "items" => self.items = Some(value),
            _ => {}
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "duration_ms" {
            self.duration = Duration::try_from_secs_f64(value / 1000.0).ok();
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}
}

impl ClientFields {
    fn label(&self, name: &str) -> String {
        let method = self.method.as_deref().unwrap_or("HTTP");
        match name {
            "hubuum.request" => format!("request {method} {}", self.route.as_deref().unwrap_or("")),
            "hubuum.http_attempt" => format!(
                "{} {method} attempt {}",
                if self.transport == Some("custom") {
                    "transport"
                } else {
                    "HTTP"
                },
                self.attempt.unwrap_or(1)
            ),
            "hubuum.retry" => format!("retry after attempt {}", self.attempt.unwrap_or(1)),
            "hubuum.stream" => "stream body".to_owned(),
            "hubuum.page" => format!(
                "page {}: {} items",
                self.page.unwrap_or(0),
                self.items.unwrap_or(0)
            ),
            _ => format!("client span {name}"),
        }
    }
}

impl CommandTrace {
    fn new(enabled: bool) -> Self {
        Self(Arc::new(Mutex::new(CommandReport {
            enabled,
            duration: None,
            spans: Vec::new(),
        })))
    }

    fn enable(&self) {
        self.0.lock().expect("debug trace lock poisoned").enabled = true;
    }

    fn finish(&self, duration: Duration) {
        self.0
            .lock()
            .expect("debug trace lock poisoned")
            .duration
            .get_or_insert(duration);
    }

    fn render(&self) -> Option<String> {
        let report = self.0.lock().expect("debug trace lock poisoned");
        if !report.enabled {
            return None;
        }
        let duration = report.duration?;
        let visible = || report.spans.iter().filter(|span| !span.empty);
        let calls = visible()
            .filter(|span| span.kind == SpanKind::ClientCall)
            .count();
        let library_spans = visible()
            .filter(|span| span.kind == SpanKind::ClientLibrary && span.client.is_none())
            .count();
        let attempts = |transport| {
            visible()
                .filter(|span| {
                    span.name == "hubuum.http_attempt"
                        && span
                            .client
                            .as_ref()
                            .is_some_and(|fields| fields.transport == Some(transport))
                })
                .count()
        };
        let mut output = format!(
            "Debug: command took {:.3} ms; {calls} client API calls",
            duration.as_secs_f64() * 1000.0,
        );
        for (transport, label) in [
            ("reqwest", "HTTP attempts"),
            ("custom", "custom transport attempts"),
        ] {
            let count = attempts(transport);
            if count > 0 {
                write!(output, "; {count} {label}").unwrap();
            }
        }
        if library_spans > 0 {
            write!(output, "; {library_spans} client library spans").unwrap();
        }
        output.push('\n');
        let mut call_number = 0;
        for span in visible() {
            write!(output, "{}", "  ".repeat(span.depth)).unwrap();
            match span.kind {
                SpanKind::ClientCall => {
                    call_number += 1;
                    write!(output, "{call_number}. ").unwrap();
                }
                SpanKind::ClientLibrary if span.client.is_none() => output.push_str("client span "),
                _ => {}
            }
            let name = span
                .client
                .as_ref()
                .map_or_else(|| span.name.clone(), |fields| fields.label(&span.name));
            if span.kind == SpanKind::Page {
                writeln!(output, "{name}").unwrap();
                continue;
            }
            write!(output, "{name}: ").unwrap();
            if let Some(duration) = span.duration {
                write!(output, "{:.3} ms", duration.as_secs_f64() * 1000.0).unwrap();
            } else {
                output.push_str("unfinished");
            }
            if let Some(fields) = &span.client {
                if let Some(status) = fields.status {
                    write!(output, " [status {status}]").unwrap();
                }
                if span.duration.is_some() {
                    if let Some(outcome) = fields.outcome {
                        write!(output, " ({outcome})").unwrap();
                    }
                }
            }
            if let Some(succeeded) = span.succeeded {
                write!(output, " ({})", if succeeded { "ok" } else { "failed" }).unwrap();
            }
            output.push('\n');
        }
        Some(output)
    }
}

struct DiagnosticsLayer;

impl<S> Layer<S> for DiagnosticsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let started = Instant::now();
        let metadata = attrs.metadata();
        if metadata.target() == TARGET && metadata.name() == "cli.command" {
            if let Ok(trace) = TRACE.try_with(Clone::clone) {
                span.extensions_mut().insert(CapturedSpan {
                    trace,
                    started,
                    index: None,
                    depth: 0,
                });
            }
            return;
        }
        // Follow the actual parent chain, including explicit parents. Task-local
        // state alone would wrongly attach detached spans to an active command.
        let parent = span.scope().skip(1).find_map(|parent| {
            parent
                .extensions()
                .get::<CapturedSpan>()
                .map(|captured| (captured.trace.clone(), captured.depth))
        });
        let Some((trace, parent_depth)) = parent else {
            return;
        };
        let kind = if metadata.target() == TARGET {
            match metadata.name() {
                "cli.stage" => SpanKind::Stage,
                "cli.client_call" => SpanKind::ClientCall,
                _ => return,
            }
        } else {
            SpanKind::ClientLibrary
        };
        let mut report = trace.0.lock().expect("debug trace lock poisoned");
        if report.duration.is_some() || (!report.enabled && kind != SpanKind::Stage) {
            return;
        }
        let mut fields = SpanFields::default();
        if kind != SpanKind::ClientLibrary {
            attrs.record(&mut fields);
        }
        let mut client = ClientFields::default();
        if metadata.target() == CLIENT_TARGET {
            attrs.record(&mut client);
        }
        let client = (client.version == Some(1)).then_some(client);
        let index = report.spans.len();
        let depth = parent_depth + 1;
        report.spans.push(TimedSpan {
            kind,
            name: fields.name.unwrap_or_else(|| metadata.name().to_owned()),
            depth,
            duration: None,
            succeeded: fields.succeeded,
            empty: fields.empty,
            client,
        });
        drop(report);
        span.extensions_mut().insert(CapturedSpan {
            trace,
            started,
            index: Some(index),
            depth,
        });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let extensions = span.extensions();
        let Some(captured) = extensions.get::<CapturedSpan>() else {
            return;
        };
        let mut fields = SpanFields::default();
        if span.metadata().target() == TARGET {
            values.record(&mut fields);
        }
        if fields.finished && captured.index.is_none() {
            captured.trace.finish(captured.started.elapsed());
            return;
        }
        let mut report = captured.trace.0.lock().expect("debug trace lock poisoned");
        if report.duration.is_some() {
            return;
        }
        if let Some(index) = captured.index {
            let entry = &mut report.spans[index];
            if let Some(client) = entry.client.as_mut() {
                values.record(client);
                entry.duration = client.duration;
            }
            if fields.finished {
                entry.duration = Some(captured.started.elapsed());
            }
            if let Some(succeeded) = fields.succeeded {
                entry.succeeded = Some(succeeded);
            }
            entry.empty |= fields.empty;
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if event.metadata().target() != CLIENT_TARGET || event.metadata().name() != "hubuum.page" {
            return;
        }
        let mut client = ClientFields::default();
        event.record(&mut client);
        if client.version != Some(1) {
            return;
        }
        let parent = ctx.event_scope(event).and_then(|mut scope| {
            scope.find_map(|span| {
                span.extensions()
                    .get::<CapturedSpan>()
                    .map(|captured| (captured.trace.clone(), captured.depth))
            })
        });
        let Some((trace, depth)) = parent else { return };
        let mut report = trace.0.lock().expect("debug trace lock poisoned");
        if !report.enabled || report.duration.is_some() {
            return;
        }
        report.spans.push(TimedSpan {
            kind: SpanKind::Page,
            name: "hubuum.page".to_owned(),
            depth: depth + 1,
            duration: None,
            succeeded: None,
            empty: false,
            client: Some(client),
        });
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let extensions = span.extensions();
        let Some(captured) = extensions.get::<CapturedSpan>() else {
            return;
        };
        let mut report = captured.trace.0.lock().expect("debug trace lock poisoned");
        if report.duration.is_some() {
            return;
        }
        if let Some(index) = captured.index {
            let entry = &mut report.spans[index];
            entry
                .duration
                .get_or_insert_with(|| captured.started.elapsed());
        } else {
            report.duration = Some(captured.started.elapsed());
        }
    }
}

pub(crate) fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    DiagnosticsLayer.with_filter(filter_fn(|metadata| {
        metadata.is_span()
            && (metadata.target() == TARGET
                || metadata.target() == "hubuum_client"
                || metadata.target().starts_with("hubuum_client::"))
            || (metadata.target() == CLIENT_TARGET && metadata.name() == "hubuum.page")
    }))
}

/// Offline commands also need a subscriber, without creating a log file.
pub(crate) fn init() {
    tracing_subscriber::registry().with(layer()).init();
}

pub(crate) fn set_default(enabled: bool) {
    DEBUG_DEFAULT.store(enabled, Ordering::Relaxed);
}

pub(crate) fn enable() {
    let _ = TRACE.try_with(CommandTrace::enable);
}

async fn run_in_trace<T>(trace: CommandTrace, operation: impl Future<Output = T>) -> T {
    TRACE
        .scope(trace, async {
            let span =
                debug_span!(target: TARGET, parent: None, "cli.command", finished = field::Empty);
            let result = operation.instrument(span.clone()).await;
            // Finish at the command boundary even if a spawned task retains a child
            // span. Late work cannot extend this report or enter the next command's.
            span.record("finished", true);
            result
        })
        .await
}

/// Reuse the enclosing command for workflows. Independent commands each get a
/// root span, and only its descendants contribute to the report.
pub(crate) async fn command<T>(operation: impl Future<Output = T>) -> T {
    if TRACE.try_with(|_| ()).is_ok() {
        return operation.await;
    }
    let trace = CommandTrace::new(DEBUG_DEFAULT.load(Ordering::Relaxed));
    let result = run_in_trace(trace.clone(), operation).await;
    if let Some(report) = trace.render() {
        eprint!("{report}");
    }
    result
}

pub(crate) async fn stage<T>(name: &'static str, operation: impl Future<Output = T>) -> T {
    let span = debug_span!(target: TARGET, "cli.stage", stage = name, finished = field::Empty);
    let result = operation.instrument(span.clone()).await;
    span.record("finished", true);
    result
}

pub(crate) fn spawn_blocking<F, R>(operation: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let trace = TRACE.try_with(Clone::clone).ok();
    let span = Span::current();
    let dispatch = dispatcher::get_default(Clone::clone);
    tokio::task::spawn_blocking(move || {
        dispatcher::with_default(&dispatch, || {
            span.in_scope(|| match trace {
                Some(trace) => TRACE.sync_scope(trace, operation),
                None => operation(),
            })
        })
    })
}

/// Emit a span around a client operation without inspecting its data or error.
pub(crate) fn client_call<T, E>(
    operation: &'static str,
    call: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    measure(operation, call, |result| Some(result.is_ok()))
}

/// Lazy page iteration has no client call when `next()` only signals exhaustion.
pub(crate) fn client_page<T, E>(
    pages: &mut impl Iterator<Item = Result<T, E>>,
) -> Option<Result<T, E>> {
    measure(
        "pages.next",
        || pages.next(),
        |page| page.as_ref().map(Result::is_ok),
    )
}

fn measure<T>(
    operation: &'static str,
    call: impl FnOnce() -> T,
    outcome: impl FnOnce(&T) -> Option<bool>,
) -> T {
    let span = debug_span!(target: TARGET, "cli.client_call", operation, succeeded = field::Empty, empty = field::Empty, finished = field::Empty);
    span.in_scope(|| {
        let result = call();
        match outcome(&result) {
            Some(succeeded) => {
                span.record("succeeded", succeeded);
            }
            None => {
                span.record("empty", true);
            }
        }
        span.record("finished", true);
        result
    })
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use tracing::instrument::WithSubscriber;
    use tracing::{debug_span, field, Dispatch, Instrument};
    use tracing_subscriber::prelude::*;

    use super::{
        client_call, client_page, command, enable, layer, run_in_trace, spawn_blocking, stage,
        CommandTrace,
    };

    fn subscriber() -> Dispatch {
        Dispatch::new(tracing_subscriber::registry().with(layer()))
    }

    #[tokio::test]
    async fn client_contract_counts_http_attempts_and_uses_reported_completion_times() {
        let trace = CommandTrace::new(true);
        let retained = run_in_trace(trace.clone(), async {
            client_call("objects.list", || {
                let request = debug_span!(target: super::CLIENT_TARGET, "hubuum.request",
                    diagnostics.version = 1_u64, http.request.method = "GET",
                    http.route = "/api/v1/classes/{class_id}/objects", outcome = field::Empty,
                    duration_ms = field::Empty, token = "private-token");
                request.in_scope(|| {
                    for (attempt, status, outcome) in [(1_u64, 503_u64, "error"), (2, 200, "ok")] {
                        let span = debug_span!(target: super::CLIENT_TARGET, "hubuum.http_attempt",
                            diagnostics.version = 1_u64, http.request.method = "GET", attempt,
                            transport = "reqwest", http.response.status_code = field::Empty,
                            outcome = field::Empty, duration_ms = field::Empty);
                        span.record("http.response.status_code", status);
                        span.record("outcome", outcome);
                        span.record("duration_ms", 2.5_f64);
                    }
                });
                request.record("outcome", "ok");
                request.record("duration_ms", 7.5_f64);
                tracing::event!(name: "hubuum.page", target: super::CLIENT_TARGET,
                    tracing::Level::DEBUG, diagnostics.version = 1_u64, page = 1_u64, items = 2_u64);
                Ok::<_, ()>(request)
            }).unwrap()
        }).with_subscriber(subscriber()).await;
        let report = trace.render().unwrap();
        assert!(
            report.contains("1 client API calls; 2 HTTP attempts"),
            "{report}"
        );
        assert!(report.contains("request GET /api/v1/classes/{class_id}/objects: 7.500 ms (ok)"));
        assert!(report.contains("HTTP GET attempt 1: 2.500 ms [status 503] (failed)"));
        assert!(report.contains("HTTP GET attempt 2: 2.500 ms [status 200] (ok)"));
        assert!(report.contains("page 1: 2 items"));
        assert!(!report.contains("private"));
        assert!(!report.contains("unfinished"));
        drop(retained);
        assert_eq!(trace.render().unwrap(), report);
    }

    #[tokio::test]
    async fn native_client_spans_need_no_wrapper_and_include_time_waiting_for_workers() {
        let trace = CommandTrace::new(true);
        let wait = Duration::from_millis(10);
        run_in_trace(trace.clone(), async {
            async {
                spawn_blocking(move || thread::sleep(wait)).await.unwrap();
            }
            .instrument(debug_span!(target: "hubuum_client", "client.operation"))
            .await;
        })
        .with_subscriber(subscriber())
        .await;
        let report = trace.render().unwrap();
        assert!(report.contains("0 client API calls; 1 client library spans"));
        let state = trace.0.lock().unwrap();
        assert!(state.duration.unwrap() >= wait);
        assert!(state.spans[0].duration.unwrap() >= wait);
    }

    #[tokio::test]
    async fn pagination_counts_yielded_pages_and_failures_but_not_exhaustion() {
        let trace = CommandTrace::new(true);
        run_in_trace(trace.clone(), async {
            let mut pages = [Ok(1), Err("failed page")].into_iter();
            assert_eq!(client_page(&mut pages), Some(Ok(1)));
            assert_eq!(client_page(&mut pages), Some(Err("failed page")));
            assert_eq!(client_page(&mut pages), None);
        })
        .with_subscriber(subscriber())
        .await;
        let report = trace.render().unwrap();
        assert!(report.contains("2 client API calls"), "{report}");
        assert!(report.contains("(failed)"));
        assert!(!report.contains("unfinished"));
    }

    #[tokio::test]
    async fn traces_blocking_calls_and_native_client_spans_without_payloads_or_background() {
        let trace = CommandTrace::new(true);
        run_in_trace(trace.clone(), stage("execute", async {
            assert_eq!(client_call("classes.get_by_name", || Ok::<_, &str>(42)), Ok(42));
            let error = spawn_blocking(|| client_call("objects.get", || {
                let span = debug_span!(target: "hubuum_client::http", "http.attempt", token = "private-token", error = field::Empty);
                span.in_scope(|| span.record("error", "private-error"));
                Err::<(), _>("secret error")
            })).await.expect("worker should finish");
            assert_eq!(error, Err("secret error"));
            tokio::spawn(async {
                client_call("background.tasks.get", || Ok::<_, ()>(())).unwrap();
            }).await.unwrap();
            debug_span!(target: "hubuum_client", parent: None, "detached").in_scope(|| {});
            command(async { client_call("nested.workflow", || Ok::<_, ()>(())).unwrap(); }).await;
        })).with_subscriber(subscriber()).await;
        let report = trace.render().expect("enabled report");
        assert!(
            report.contains("3 client API calls; 1 client library spans"),
            "{report}"
        );
        assert!(report.contains("  execute:"));
        assert!(report.contains("    1. classes.get_by_name:"));
        assert!(report.contains("    2. objects.get:"));
        assert!(report.contains("      client span http.attempt:"));
        assert!(report.contains("    3. nested.workflow:"));
        assert!(report.contains("(failed)"));
        for excluded in ["secret", "private", "background", "detached", "unfinished"] {
            assert!(!report.contains(excluded), "{report}");
        }
    }

    #[tokio::test]
    async fn disabled_sequential_and_concurrent_commands_keep_separate_reports() {
        let dispatch = subscriber();
        let disabled = CommandTrace::new(false);
        run_in_trace(disabled.clone(), async {
            client_call("hidden", || Ok::<_, ()>(())).unwrap();
        })
        .with_subscriber(dispatch.clone())
        .await;
        assert!(disabled.render().is_none());

        let first = CommandTrace::new(true);
        let second = CommandTrace::new(false);
        async {
            tokio::join!(
                run_in_trace(first.clone(), async {
                    spawn_blocking(|| client_call("first", || Ok::<_, ()>(())))
                        .await
                        .unwrap()
                        .unwrap();
                }),
                run_in_trace(
                    second.clone(),
                    stage("execute", async {
                        enable();
                        spawn_blocking(|| client_call("second", || Ok::<_, ()>(())))
                            .await
                            .unwrap()
                            .unwrap();
                    })
                ),
            );
        }
        .with_subscriber(dispatch.clone())
        .await;
        assert!(!first.render().unwrap().contains("second"));
        let second_report = second.render().unwrap();
        assert!(!second_report.contains("first"));
        assert!(second_report.contains("execute:"));

        let third = CommandTrace::new(true);
        run_in_trace(third.clone(), async {})
            .with_subscriber(dispatch)
            .await;
        assert!(third.render().unwrap().contains("0 client API calls"));
    }

    #[tokio::test]
    async fn retained_spans_cannot_change_a_finished_report_or_leak_into_the_next_command() {
        let dispatch = subscriber();
        let first = CommandTrace::new(true);
        let retained = run_in_trace(first.clone(), async {
            debug_span!(target: "hubuum_client", "retained")
        })
        .with_subscriber(dispatch.clone())
        .await;
        let snapshot = first.render().unwrap();
        assert!(snapshot.contains("client span retained: unfinished"));
        let second = CommandTrace::new(true);
        run_in_trace(second.clone(), async {
            async {
                debug_span!(target: "hubuum_client", "late_child").in_scope(|| {});
            }
            .instrument(retained)
            .await;
            client_call("current", || Ok::<_, ()>(())).unwrap();
        })
        .with_subscriber(dispatch)
        .await;
        assert_eq!(first.render().unwrap(), snapshot);
        let report = second.render().unwrap();
        assert!(report.contains("1 client API calls"));
        assert!(!report.contains("late_child"));
        assert!(!report.contains("retained"));
    }
}
