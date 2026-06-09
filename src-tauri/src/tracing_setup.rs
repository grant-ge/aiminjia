//! Tracing subscriber setup — file logging with OTel-style trace/span IDs.
//!
//! Log line format:
//!   `[2026-06-07 10:58:50][INFO][module::path:42][trace=<id>] message`
//!   `[2026-06-07 10:58:50][INFO][module::path:42][trace=<id> span=<id>] message`
//!
//! Every log line always has a `[trace=…]` field:
//!   - Inside a turn (chat request): trace = run_id (UUID without dashes)
//!   - Inside a sub-agent: trace = run_id + span = first 16 hex of agent_id
//!   - Background / startup: trace = app-<8hex> generated once at launch
//!
//! The subscriber accepts all tracing levels; actual level filtering is done by
//! `log::set_max_level()` which gates `log::*` macros before they reach tracing.

use std::path::Path;
use std::sync::OnceLock;

use tracing::{Event, Subscriber};
use tracing_subscriber::{
    filter::Targets,
    fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields},
    layer::{Context, Layer, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
};

// ---------------------------------------------------------------------------
// SkyWalking-inspired three-segment trace context
// ---------------------------------------------------------------------------
//
// Trace ID format: `{instance}.{epoch_secs}.{seq:05d}`
//   - instance  : 8 hex chars, randomly generated once per app launch (cross-machine unique)
//   - epoch_secs: Unix timestamp in seconds at trace creation (human-readable time anchor)
//   - seq       : 5-digit zero-padded counter, wraps at 99999 (unique within same second)
//
// Example: `a3ce929d.1717728512.00003`
//
// Span ID format (child spans only): `{seq:05d}` — 5-digit counter, unique within a trace.
// Example: `00002`
//
// Global uniqueness: instance (2^32 random) × timestamp (second precision) × seq (99 999/s)
// makes accidental collision across machines, restarts, or concurrent users astronomically rare.

/// Randomly generated once per app launch; stored in the first segment of every trace ID.
static APP_INSTANCE_ID: OnceLock<String> = OnceLock::new();

fn app_instance_id() -> &'static str {
    APP_INSTANCE_ID.get_or_init(|| {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
    })
}

// ---------------------------------------------------------------------------
// Clock-rollback-safe ID generator
// ---------------------------------------------------------------------------
//
// Holds the last emitted millisecond timestamp and a per-millisecond sequence
// counter under a single Mutex so both are updated atomically.
//
// Clock rollback protection (Snowflake-style):
//   If SystemTime goes backward (NTP correction, manual clock change), we reuse
//   the last known timestamp and keep incrementing the sequence — ensuring IDs
//   always move forward and never duplicate.

struct IdState {
    last_millis: u64,
    seq: u32, // wraps at 99 999 → always 5 digits
}

static ID_STATE: std::sync::Mutex<IdState> = std::sync::Mutex::new(IdState {
    last_millis: 0,
    seq: 99_999, // first call: (99_999 + 1) % 100_000 = 0
});

/// Returns `(millisecond_timestamp, sequence)` with clock-rollback protection.
fn next_id_parts() -> (u64, u32) {
    let mut state = ID_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let millis = now.max(state.last_millis);
    state.last_millis = millis;
    state.seq = (state.seq + 1) % 100_000;
    (millis, state.seq)
}

/// Advance the global seq to `max(current, remote_seq)` so the next local
/// span continues from wherever the server left off.  Called by the HTTP
/// middleware after parsing the response `X-Span-Id`.
pub fn advance_seq_to(remote_seq: u32) {
    let mut state = ID_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if remote_seq >= state.seq {
        state.seq = remote_seq; // next call to next_id_parts() will +1 from here
    }
}

/// New root trace ID: `{instance}.{epoch_ms}.{seq:05d}`
fn new_trace_id() -> String {
    let (ms, seq) = next_id_parts();
    format!("{}.{ms}.{seq:05}", app_instance_id())
}

/// New span ID: `{seq:05d}` — child spans only, unique within one trace.
fn new_span_id() -> String {
    let (_, seq) = next_id_parts();
    format!("{seq:05}")
}

/// Per-span trace context stored in span extensions.
struct OtelContext {
    trace_id: String, // shared across a trace
    span_id: String,  // this span's sequential ID
}

/// Layer that assigns OTel-compatible trace_id / span_id to every span.
pub struct TraceContextLayer;

impl<S> Layer<S> for TraceContextLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).unwrap();

        // Inherit trace_id from parent if one exists; otherwise start a new trace.
        // attrs.parent() gives an explicit parent; fallback to the current ambient span.
        let parent_trace_id = attrs
            .parent()
            .and_then(|pid| ctx.span(pid))
            .or_else(|| ctx.lookup_current())
            .and_then(|p| p.extensions().get::<OtelContext>().map(|c| c.trace_id.clone()));
        let trace_id = parent_trace_id.unwrap_or_else(new_trace_id);

        span.extensions_mut().insert(OtelContext {
            trace_id,
            span_id: new_span_id(),
        });
    }
}

// ---------------------------------------------------------------------------
// App-level fallback trace ID — set once after the subscriber is initialized.
// Holds the hex-formatted tracing::Id of the global "app" span so background
// log lines show the same 16-hex format as turn/agent spans.
static APP_TRACE_ID: OnceLock<String> = OnceLock::new();

fn app_trace_id() -> &'static str {
    APP_TRACE_ID.get().map(|s| s.as_str()).unwrap_or("0000000000000000")
}

// ---------------------------------------------------------------------------
// AijiaLogFormat — custom FormatEvent
// ---------------------------------------------------------------------------

pub struct AijiaLogFormat;

impl<S, N> FormatEvent<S, N> for AijiaLogFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let ts = chrono::Local::now().format("[%Y-%m-%d %H:%M:%S]");

        // Visit fields first: for log::* macros bridged via tracing-log, the real
        // caller target/line are in log.target / log.line fields, not in the
        // tracing metadata (which just shows "log" / 0 from the bridge callsite).
        let mut v = MessageVisitor::default();
        event.record(&mut v);

        let target = v.log_target.as_deref().unwrap_or_else(|| meta.target());
        let line = v.log_line.unwrap_or_else(|| meta.line().unwrap_or(0));

        let trace_prefix = ctx
            .lookup_current()
            .and_then(|span| resolve_trace_prefix(&span))
            .unwrap_or_else(|| format!("[trace={}]", app_trace_id()));

        write!(writer, "{}[{}][{}:{line}]{trace_prefix} ", ts, meta.level(), target)?;
        write!(writer, "{}", v.message)?;
        writeln!(writer)
    }
}

/// Build `[trace=…]` / `[trace=… span=…]` from `OtelContext` span extensions.
///
/// All spans in one trace share the same `trace_id`; only child spans additionally
/// show `span_id` so you can distinguish sub-agents from the parent turn.
fn resolve_trace_prefix<S>(span: &tracing_subscriber::registry::SpanRef<'_, S>) -> Option<String>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let ext = span.extensions();
    let ctx = ext.get::<OtelContext>()?;
    let trace_id = ctx.trace_id.clone();
    let span_id = ctx.span_id.clone();
    drop(ext);
    Some(format!("[trace={trace_id} span={span_id}]"))
}

/// Visits event fields and assembles the log message string.
///
/// For `log::*` macros bridged via tracing-log, the bridge stores the original
/// caller's location as fields (`log.target`, `log.line`, `log.file`, `log.module_path`).
/// We capture `log.target` and `log.line` so the formatter can show the real source
/// location instead of `"log":0` from the bridge callsite.
#[derive(Default)]
struct MessageVisitor {
    message: String,
    /// Original `log::Record::target()` — the module path of the log::* call site.
    log_target: Option<String>,
    /// Original `log::Record::line()` — the line number of the log::* call site.
    log_line: Option<u32>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "log.target" => self.log_target = Some(value.to_string()),
            // log.file / log.module_path — skip, not needed in output
            _ if field.name().starts_with("log.") => {}
            _ => {
                if !self.message.is_empty() {
                    self.message.push(' ');
                }
                self.message.push_str(&format!("{}={}", field.name(), value));
            }
        }
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "log.line" {
            self.log_line = Some(value as u32);
        }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            // DisplayValue from tracing-log implements Debug as Display — no quotes.
            "message" => self.message = format!("{value:?}"),
            _ if field.name().starts_with("log.") => {}
            _ => {
                if !self.message.is_empty() {
                    self.message.push(' ');
                }
                self.message.push_str(&format!("{}={:?}", field.name(), value));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// init — call once at app startup
// ---------------------------------------------------------------------------

/// Initialize the global tracing subscriber and bridge `log::*` macros through it.
///
/// Log files are written to `<logs_dir>/renlijia.<YYYY-MM-DD>` with daily rotation.
/// Old files are cleaned up by the existing `cleanup_old_logs()` in lib.rs.
///
/// Level gate: the subscriber accepts trace+ so every `log::*` macro call reaches it;
/// the actual verbosity cutoff is `log::set_max_level()` applied separately (see log_level.rs).
pub fn init(logs_dir: &Path) {
    app_trace_id(); // set once before any log line fires

    let file_appender = tracing_appender::rolling::daily(logs_dir, "renlijia");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // Guard must live for the whole process — leak it.
    Box::leak(Box::new(guard));

    // Third-party crates that use native tracing::* macros bypass log::set_max_level().
    // Suppress their verbose levels here so they don't pollute the log file.
    // Our own app_lib code is left at TRACE (gated instead by log::set_max_level()).
    let crate_filter = Targets::new()
        .with_default(tracing::Level::TRACE) // app_lib and unknown crates: pass through
        .with_target("hyper", tracing::Level::WARN)
        .with_target("hyper_util", tracing::Level::WARN)
        .with_target("tokio", tracing::Level::WARN)
        .with_target("mio", tracing::Level::WARN)
        .with_target("want", tracing::Level::WARN)
        .with_target("h2", tracing::Level::WARN)
        .with_target("tower", tracing::Level::WARN)
        .with_target("tungstenite", tracing::Level::WARN)
        .with_target("tokio_tungstenite", tracing::Level::WARN)
        .with_target("rustls", tracing::Level::WARN);

    // tracing-subscriber's default features include `tracing-log`, so `.init()` also
    // calls LogTracer::init() internally — routing all log::* macros through tracing.
    // Do NOT call LogTracer::init() manually; that would panic with SetLoggerError.
    let registry = tracing_subscriber::registry()
        .with(crate_filter)
        .with(TraceContextLayer)
        .with(
            fmt::Layer::new()
                .event_format(AijiaLogFormat)
                .with_writer(non_blocking)
                .with_ansi(false),
        );

    // In debug builds also print to stderr so `pnpm tauri:dev` shows logs in the terminal.
    #[cfg(debug_assertions)]
    {
        registry
            .with(
                fmt::Layer::new()
                    .event_format(AijiaLogFormat)
                    .with_writer(std::io::stderr)
                    .with_ansi(true),
            )
            .init();
    }
    #[cfg(not(debug_assertions))]
    {
        registry.init();
    }

    // Create the global "app" span AFTER the subscriber is live so it gets a real Id.
    // Background code that has no per-request span falls back to this Id in the formatter,
    // keeping the log format uniformly [trace=<16hex>] everywhere.
    //
    // Box::leak gives the span a 'static lifetime so we can enter() it permanently on the
    // main thread (sync startup / Tauri setup code). Tokio-spawned tasks don't inherit
    // entered spans; they hit the stored fallback below instead.
    let app_span: &'static tracing::Span = Box::leak(Box::new(tracing::info_span!("app")));
    // TraceContextLayer assigned an OtelContext to this span; store its trace_id as fallback.
    let id_str = app_span
        .with_subscriber(|(id, sub)| {
            sub.downcast_ref::<tracing_subscriber::Registry>()
                .and_then(|reg| {
                    use tracing_subscriber::registry::LookupSpan;
                    reg.span(id)?.extensions().get::<OtelContext>().map(|c| c.trace_id.clone())
                })
        })
        .flatten()
        .unwrap_or_else(new_trace_id);
    APP_TRACE_ID.set(id_str).ok();

    // Enter on the main thread. std::mem::forget keeps it entered for the process lifetime.
    std::mem::forget(app_span.enter());
}

// ---------------------------------------------------------------------------
// HTTP trace header middleware + injection
// ---------------------------------------------------------------------------

/// Read (trace_id, span_id) from the current tracing span's OtelContext.
/// Public so shell tools can inject them as environment variables for child processes.
pub fn current_span_context() -> Option<(String, String)> {
    current_trace_context()
}

fn current_trace_context() -> Option<(String, String)> {
    let span = tracing::Span::current();
    if span.is_disabled() {
        return None;
    }
    span.with_subscriber(|(id, dispatch)| {
        use tracing_subscriber::registry::LookupSpan;
        dispatch
            .downcast_ref::<tracing_subscriber::Registry>()
            .and_then(|reg| reg.span(id))
            .and_then(|span_ref| {
                let ext = span_ref.extensions();
                ext.get::<OtelContext>()
                    .map(|ctx| (ctx.trace_id.clone(), ctx.span_id.clone()))
            })
    })
    .flatten()
}

/// reqwest-middleware that:
///   1. Injects `X-Trace-Id` and `X-Span-Id` into every outgoing request.
///   2. Reads `X-Span-Id` from the response and advances the global seq counter
///      so subsequent local spans continue from the server's span number.
pub struct TraceHeaderMiddleware;

#[async_trait::async_trait]
impl reqwest_middleware::Middleware for TraceHeaderMiddleware {
    async fn handle(
        &self,
        mut req: reqwest::Request,
        extensions: &mut http::Extensions,
        next: reqwest_middleware::Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        if let Some((trace_id, span_id)) = current_trace_context() {
            let h = req.headers_mut();
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&trace_id) {
                h.insert("x-trace-id", v);
            }
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&span_id) {
                h.insert("x-span-id", v);
            }
        }

        let resp = next.run(req, extensions).await?;

        // Parse the server's span ID and advance our seq so the next local span
        // continues from max(local, remote), keeping the numbering monotonic.
        let server_span_id = resp.headers()
            .get("x-span-id")
            .or_else(|| resp.headers().get("x-request-id"))
            .or_else(|| resp.headers().get("x-lotus-request-id"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());

        if let Some(remote_seq) = server_span_id {
            advance_seq_to(remote_seq);
        }

        Ok(resp)
    }
}

/// Wrap a plain `reqwest::Client` with the trace-header middleware.
/// Use this everywhere a backend HTTP client is constructed.
pub fn traced_client(inner: reqwest::Client) -> reqwest_middleware::ClientWithMiddleware {
    reqwest_middleware::ClientBuilder::new(inner)
        .with(TraceHeaderMiddleware)
        .build()
}

// ---------------------------------------------------------------------------
// current_log_file — used by diagnostics to locate today's log file
// ---------------------------------------------------------------------------

/// Return the path of today's active log file, e.g. `<logs_dir>/renlijia.2026-06-07`.
pub fn current_log_file(logs_dir: &Path) -> std::path::PathBuf {
    let date = chrono::Local::now().format("%Y-%m-%d");
    logs_dir.join(format!("renlijia.{date}"))
}
