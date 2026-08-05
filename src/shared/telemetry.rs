// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 启动期装配全局 tracing subscriber。
//!
//! 日志层风格对齐 `match-engine-fabric/src/infra/logger`：通过 [`LoggerBuilder`]
//! 流式构造，支持 Text / JSON 两种 formatter 与 Console / File（按时间轮转）
//! 两种 sink；写入路径走 `tracing-appender` 的 non-blocking writer，因此返回的
//! [`LoggerGuard`] 必须在应用生命周期内保活，否则后台写线程被 drop、日志会丢。
//!
//! Trace 外发统一由 tail-sampler 后的 `telemetry.trace.external` pipeline 管理，
//! 本模块不安装旁路 OTLP exporter。

use std::sync::{Arc, OnceLock};

use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_log::{AsLog, LogTracer};
use tracing_subscriber::{EnvFilter, Layer as _, fmt, prelude::*};

use crate::shared::{
    metrics::SqlxPoolAcquireMetricsLayer,
    self_telemetry::{SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer},
};

/// subscriber 仅装配一次（多 role 同进程共享）。
static SUBSCRIBER_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// DataFusion 的 Top-K aggregate 在 Trace 日志开启时会把 RecordBatch 原样写到
/// stdout。在 `log` 桥接入口屏蔽该模块，使它的 `log_enabled!(Trace)` 守卫始终为
/// false，同时保留其他 DataFusion 日志。
const DATAFUSION_TOPK_LOG_TARGET: &str = "datafusion_physical_plan::aggregates::topk_stream";

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

/// 日志格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// 文件轮转策略。
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

/// 日志输出目标。
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

/// 完整日志与 self-telemetry 初始化参数。
pub struct FullTelemetryInit<'a> {
    pub log_level: &'a str,
    pub format: LogFormat,
    pub output: LogOutput,
    pub trace_filter: &'a str,
    pub self_telemetry: Option<SelfTelemetryInit>,
}

/// 持有 non-blocking writer 的 guard。
/// 在此值被 drop 之前，后台写入线程会持续运行并保证日志刷新。
/// 应用必须在整个生命周期内保持该值存活。
pub struct LoggerGuard {
    _writer: WorkerGuard,
    self_telemetry: Option<Arc<SelfTelemetryHub>>,
}

impl LoggerGuard {
    /// 启动早期创建的 bounded self-telemetry bridge。bootstrap 在管理组织和系统流
    /// 准备完成后取得它并绑定异步 runtime；在此之前 callback 只写有界内存队列。
    pub fn self_telemetry(&self) -> Option<Arc<SelfTelemetryHub>> {
        self.self_telemetry.clone()
    }
}

/// 日志与 self-telemetry 初始化构建器。
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

    /// 设置日志级别。优先级低于环境变量 `RUST_LOG`；接受 `"info"` 这类简写或
    /// 完整 `EnvFilter` 指令（如 `"info,sqlx=warn"`）。
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.level = level.into();
        self
    }

    /// 设置只作用于 Span capture/export 的 filter；不读取或修改 `RUST_LOG`。
    pub fn trace_filter(mut self, filter: impl Into<String>) -> Self {
        self.trace_filter = filter.into();
        self
    }

    /// 安装进程内 self-telemetry hook。这里只创建 bounded queue；DB/网络 worker
    /// 必须在 bootstrap 完成后通过 guard 返回的 hub 激活。
    pub fn self_telemetry(mut self, init: SelfTelemetryInit) -> Self {
        self.self_telemetry = Some(init);
        self
    }

    /// 初始化全局 subscriber。返回的 [`LoggerGuard`] 必须保活直到进程退出。
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
                    let self_log_layer = subscriber_self_telemetry
                        .clone()
                        .map(|hub| SelfTelemetryLayer::logs_filtered(hub, log_filter()));
                    install_global_subscriber(
                        tracing_subscriber::registry()
                            .with(SqlxPoolAcquireMetricsLayer.with_filter(
                                tracing_subscriber::filter::filter_fn(|metadata| {
                                    metadata.target() == "sqlx::pool::acquire"
                                }),
                            ))
                            .with(self_trace_layer)
                            .with(self_log_layer)
                            .with(
                                fmt::layer()
                                    .with_writer(writer)
                                    .with_thread_ids(false)
                                    .with_thread_names(true)
                                    .with_target(true)
                                    .with_level(true)
                                    .with_line_number(true)
                                    .with_ansi(is_console)
                                    .with_filter(log_filter()),
                            ),
                    )
                }
                LogFormat::Json => {
                    let self_trace_layer = subscriber_self_telemetry
                        .clone()
                        .map(|hub| SelfTelemetryLayer::traces(hub).with_filter(span_filter()));
                    let self_log_layer = subscriber_self_telemetry
                        .clone()
                        .map(|hub| SelfTelemetryLayer::logs_filtered(hub, log_filter()));
                    install_global_subscriber(
                        tracing_subscriber::registry()
                            .with(SqlxPoolAcquireMetricsLayer.with_filter(
                                tracing_subscriber::filter::filter_fn(|metadata| {
                                    metadata.target() == "sqlx::pool::acquire"
                                }),
                            ))
                            .with(self_trace_layer)
                            .with(self_log_layer)
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

/// 旧版兼容入口：仅日志，无 OTLP，Console 输出。
pub fn init(log_level: &str, json_format: bool) -> anyhow::Result<LoggerGuard> {
    LoggerBuilder::new()
        .level(log_level)
        .trace_filter(log_level)
        .format(if json_format {
            LogFormat::Json
        } else {
            LogFormat::Text
        })
        .init()
}

/// 完整 init，并额外安装 bounded self-telemetry callback bridge。
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
