// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Assembles the global tracing subscriber at startup.
//!
//! The log layer is built fluently through [`LoggerBuilder`] and supports two formatters
//! (Text / JSON) and two sinks (Console / File, rotated by time). Writes go through the
//! non-blocking writer of `tracing-appender`, so the returned [`LoggerGuard`] must be kept
//! alive for the application's lifetime; otherwise the background writer thread is dropped
//! and logs are lost.
//!
//! All outbound trace export is managed by the `telemetry.trace.external` pipeline after the
//! tail-sampler; this module installs no bypass OTLP exporter.

use std::sync::{Arc, OnceLock};

use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_log::{AsLog, LogTracer};
use tracing_subscriber::{
    EnvFilter, Layer as _,
    filter::{FilterExt as _, filter_fn},
    fmt,
    prelude::*,
};

use crate::shared::{
    metrics::SqlxPoolAcquireMetricsLayer,
    self_telemetry::{SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer},
};

/// The subscriber is set up only once (shared by multiple roles in the same process).
static SUBSCRIBER_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// DataFusion's Top-K aggregate writes RecordBatches to stdout verbatim when Trace logging is
/// enabled. The `log` bridge entry silences that module so that its `log_enabled!(Trace)`
/// guard is always false, while other DataFusion logs are kept.
const DATAFUSION_TOPK_LOG_TARGET: &str = "datafusion_physical_plan::aggregates::topk_stream";

fn is_log_event(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.is_event()
}

fn safe_log_tracer() -> tracing_log::log_tracer::Builder {
    LogTracer::builder()
        .ignore_crate(DATAFUSION_TOPK_LOG_TARGET)
        .with_max_level(tracing::level_filters::LevelFilter::current().as_log())
}

fn install_global_subscriber<S>(subscriber: S) -> Result<(), String>
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| format!("failed to set global tracing subscriber: {error}"))?;

    safe_log_tracer()
        .init()
        .map_err(|error| format!("failed to install log compatibility tracer: {error}"))
}

/// Log format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// File rotation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRotation {
    Minutely,
    Hourly,
    Daily,
    Never,
}

impl From<FileRotation> for Rotation {
    fn from(r: FileRotation) -> Self {
        match r {
            FileRotation::Minutely => Rotation::MINUTELY,
            FileRotation::Hourly => Rotation::HOURLY,
            FileRotation::Daily => Rotation::DAILY,
            FileRotation::Never => Rotation::NEVER,
        }
    }
}

/// Log output target.
#[derive(Debug, Clone)]
pub enum LogOutput {
    Console,
    File {
        directory: String,
        file_name_prefix: String,
        rotation: FileRotation,
        max_log_files: usize,
    },
}

/// Initialization parameters for full logging and self-telemetry.
pub struct FullTelemetryInit<'a> {
    pub log_level: &'a str,
    pub format: LogFormat,
    pub output: LogOutput,
    pub trace_filter: &'a str,
    pub self_telemetry: Option<SelfTelemetryInit>,
}

/// Guard that holds the non-blocking writer.
/// Until this value is dropped, the background writer thread keeps running and guarantees that
/// logs are flushed.
/// The application must keep this value alive for its entire lifetime.
pub struct LoggerGuard {
    _writer: WorkerGuard,
    self_telemetry: Option<Arc<SelfTelemetryHub>>,
}

impl LoggerGuard {
    /// The bounded self-telemetry bridge created early in startup. Once the management
    /// organization and system streams are ready, bootstrap obtains it and binds the async
    /// runtime; until then the callback only writes to a bounded in-memory queue.
    pub fn self_telemetry(&self) -> Option<Arc<SelfTelemetryHub>> {
        self.self_telemetry.clone()
    }
}

/// Builder for logging and self-telemetry initialization.
pub struct LoggerBuilder {
    format: LogFormat,
    output: LogOutput,
    level: String,
    trace_filter: String,
    self_telemetry: Option<SelfTelemetryInit>,
}

impl Default for LoggerBuilder {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            output: LogOutput::Console,
            level: "info".to_string(),
            trace_filter: "info".to_string(),
            self_telemetry: None,
        }
    }
}

impl LoggerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn format(mut self, format: LogFormat) -> Self {
        self.format = format;
        self
    }

    pub fn output(mut self, output: LogOutput) -> Self {
        self.output = output;
        self
    }

    /// Sets the log level. Has lower priority than the `RUST_LOG` environment variable; accepts
    /// a shorthand such as `"info"` or a full `EnvFilter` directive (e.g. `"info,sqlx=warn"`).
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.level = level.into();
        self
    }

    /// Sets a filter for span capture/export only; does not read or modify `RUST_LOG`.
    pub fn trace_filter(mut self, filter: impl Into<String>) -> Self {
        self.trace_filter = filter.into();
        self
    }

    /// Installs the in-process self-telemetry hook. This only creates the bounded queue; the
    /// DB/network workers must be activated after bootstrap completes, through the hub returned
    /// by the guard.
    pub fn self_telemetry(mut self, init: SelfTelemetryInit) -> Self {
        self.self_telemetry = Some(init);
        self
    }

    /// Initializes the global subscriber. The returned [`LoggerGuard`] must be kept alive until
    /// the process exits.
    pub fn init(self) -> anyhow::Result<LoggerGuard> {
        let is_console = matches!(&self.output, LogOutput::Console);
        let (writer, worker_guard) = make_writer(&self.output);
        let self_telemetry = self.self_telemetry.map(SelfTelemetryHub::new);

        let level = self.level;
        let trace_filter = self.trace_filter;
        EnvFilter::try_new(&trace_filter)
            .map_err(|error| anyhow::anyhow!("invalid telemetry.trace.filter: {error}"))?;
        let format = self.format;
        let subscriber_self_telemetry = self_telemetry.clone();

        let initialization = SUBSCRIBER_INIT.get_or_init(move || {
            let log_filter = || {
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.clone()))
            };

            let span_filter =
                || EnvFilter::try_new(trace_filter.clone()).expect("Trace filter prevalidated");

            match format {
                LogFormat::Text => {
                    let self_trace_layer = subscriber_self_telemetry
                        .clone()
                        .map(|hub| SelfTelemetryLayer::traces(hub).with_filter(span_filter()));

                    install_global_subscriber(
                        tracing_subscriber::registry()
                            .with(SqlxPoolAcquireMetricsLayer.with_filter(
                                tracing_subscriber::filter::filter_fn(|metadata| {
                                    metadata.target() == "sqlx::pool::acquire"
                                }),
                            ))
                            .with(self_trace_layer)
                            .with(
                                fmt::layer()
                                    .with_writer(writer)
                                    .with_thread_ids(false)
                                    .with_thread_names(true)
                                    .with_target(true)
                                    .with_level(true)
                                    .with_line_number(true)
                                    .with_ansi(is_console)
                                    // Text logs already carry target, line and event fields. Keep
                                    // span capture in the telemetry layer without repeating the
                                    // complete span scope before every human-readable message.
                                    .with_filter(log_filter().and(filter_fn(is_log_event))),
                            ),
                    )
                }
                LogFormat::Json => {
                    let self_trace_layer = subscriber_self_telemetry
                        .clone()
                        .map(|hub| SelfTelemetryLayer::traces(hub).with_filter(span_filter()));

                    install_global_subscriber(
                        tracing_subscriber::registry()
                            .with(SqlxPoolAcquireMetricsLayer.with_filter(
                                tracing_subscriber::filter::filter_fn(|metadata| {
                                    metadata.target() == "sqlx::pool::acquire"
                                }),
                            ))
                            .with(self_trace_layer)
                            .with(
                                fmt::layer()
                                    .json()
                                    .with_writer(writer)
                                    .with_thread_ids(false)
                                    .with_thread_names(true)
                                    .with_target(true)
                                    .with_level(true)
                                    .with_line_number(true)
                                    .with_ansi(false)
                                    .with_filter(log_filter()),
                            ),
                    )
                }
            }
        });

        if let Err(error) = initialization {
            anyhow::bail!(error.clone());
        }

        Ok(LoggerGuard {
            _writer: worker_guard,
            self_telemetry,
        })
    }
}

fn make_writer(output: &LogOutput) -> (NonBlocking, WorkerGuard) {
    match output {
        LogOutput::Console => tracing_appender::non_blocking(std::io::stdout()),
        LogOutput::File {
            directory,
            file_name_prefix,
            rotation,
            max_log_files,
        } => {
            let file_appender = RollingFileAppender::builder()
                .rotation((*rotation).into())
                .max_log_files(*max_log_files)
                .filename_prefix(file_name_prefix)
                .filename_suffix("log")
                .build(directory)
                .expect("create log file appender failed");

            tracing_appender::non_blocking(file_appender)
        }
    }
}

/// Full init that additionally installs a bounded self-telemetry callback bridge.
pub fn init_full_with_self_telemetry(init: FullTelemetryInit<'_>) -> anyhow::Result<LoggerGuard> {
    let FullTelemetryInit {
        log_level,
        format,
        output,
        trace_filter,
        self_telemetry,
    } = init;

    let mut builder = LoggerBuilder::new()
        .level(log_level)
        .trace_filter(trace_filter)
        .format(format)
        .output(output);

    if let Some(init) = self_telemetry {
        builder = builder.self_telemetry(init);
    }

    builder.init()
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::filter::LevelFilter;

    use super::*;

    #[derive(Clone)]
    struct BufferWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for BufferWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut output = self
                .0
                .lock()
                .expect("log buffer lock should not be poisoned");
            output.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn text_logs_omit_contextual_span_scope() {
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer_output = std::sync::Arc::clone(&output);
        let subscriber = tracing_subscriber::registry()
            // Mimic the telemetry layer keeping the span enabled while the text layer ignores it.
            .with(LevelFilter::TRACE)
            .with(
                fmt::layer()
                    .without_time()
                    .with_ansi(false)
                    .with_target(false)
                    .with_thread_names(false)
                    .with_writer(move || BufferWriter(std::sync::Arc::clone(&writer_output)))
                    .with_filter(EnvFilter::new("trace").and(filter_fn(is_log_event))),
            );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "worker.wal_replay",
                otel.kind = "internal",
                molesignal.worker.name = "wal_replay"
            );
            let _entered = span.enter();
            tracing::info!("intake wal replay starting");
        });

        let output = String::from_utf8(
            output
                .lock()
                .expect("log buffer lock should not be poisoned")
                .clone(),
        )
        .expect("formatted log should be UTF-8");
        assert!(output.contains("intake wal replay starting"));
        assert!(!output.contains("worker.wal_replay"), "{output}");
        assert!(!output.contains("molesignal.worker.name"), "{output}");
    }

    #[test]
    fn datafusion_topk_batches_are_never_log_enabled() {
        let subscriber = tracing_subscriber::registry().with(LevelFilter::TRACE);
        let _subscriber_guard = tracing::subscriber::set_default(subscriber);
        safe_log_tracer()
            .init()
            .expect("test process should not have another global logger");

        let blocked = log::Metadata::builder()
            .level(log::Level::Trace)
            .target(DATAFUSION_TOPK_LOG_TARGET)
            .build();
        assert!(!log::logger().enabled(&blocked));

        let ordinary_datafusion_log = log::Metadata::builder()
            .level(log::Level::Trace)
            .target("datafusion_physical_plan::aggregates::other")
            .build();
        assert!(log::logger().enabled(&ordinary_datafusion_log));
    }
}
