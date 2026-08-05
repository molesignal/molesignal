// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use molesignal::bootstrap::{activate_self_telemetry, build_state, rewrap_kek, roles};

#[derive(Parser, Debug)]
#[command(name = "molesignal", version, about = "MoleSignal server")]
struct Cli {
    /// 配置文件路径（TOML）。
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 离线轮换 KEK：用旧 KEK 解、当前 `MS_CIPHER_KEY` 重封全部 cipher-key 信封列。
    /// 停写窗口执行，跑完核对各表计数无误后再下线旧 KEK。
    RewrapKek {
        /// 旧 KEK（base64 标准编码 32 字节）。
        #[arg(long)]
        old_key: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 把烘进的 jemalloc `_rjem_malloc_conf` 符号显式拉进最终链接（见 allocator.rs）；纯链接
    // 保留用途、无运行期副作用（jemalloc 在 main 前就已读取该符号）。非 Linux/非 jemalloc 为 no-op。
    molesignal::bootstrap::allocator::ensure_malloc_conf_linked();

    let cli = Cli::parse();
    let settings = molesignal::config::load(cli.config.as_deref())?;

    // LoggerGuard 必须保活到 main 退出，否则后台非阻塞写线程被 drop、文件日志会丢。
    // Capture is shared by both retained-trace sinks. Disabling self-ingest must
    // not silently disable an independently configured external OTLP exporter.
    let trace_capture_enabled = settings.telemetry.trace.effective_enabled();
    let self_telemetry_enabled = settings.telemetry.self_collect.enabled;
    let self_telemetry_init = (self_telemetry_enabled || trace_capture_enabled).then(|| {
        use molesignal::shared::self_telemetry::{ResourceIdentity, SelfTelemetryInit};
        let roles = settings
            .node
            .roles
            .iter()
            .map(role_str)
            .collect::<Vec<_>>()
            .join(",");
        SelfTelemetryInit {
            queue_capacity: settings.telemetry.self_collect.queue_capacity,
            logs_enabled: self_telemetry_enabled,
            traces_enabled: trace_capture_enabled,
            resource: ResourceIdentity::new(
                "molesignal",
                env!("CARGO_PKG_VERSION"),
                settings.telemetry.trace.deployment_environment.clone(),
                roles,
                settings.node.id.clone(),
            ),
        }
    });
    let logger_guard = molesignal::shared::telemetry::init_full_with_self_telemetry(
        molesignal::shared::telemetry::FullTelemetryInit {
            log_level: &settings.telemetry.log_level,
            format: parse_log_format(&settings.telemetry.log_format),
            output: build_log_output(&settings.telemetry),
            trace_filter: &settings.telemetry.trace.filter,
            self_telemetry: self_telemetry_init,
        },
    )?;

    // config 文件 hot-reload watcher。
    // 当前 apply 路径仅打 log（mutable 字段如 telemetry.log_level / notify.* 的真实
    // 热应用闭包待 follow-up，按字段按需挂上）。
    // 把 watcher 保活的 Arc 泄到 'static 防止被 drop；watcher 自身轻量。
    let _watcher_guard: Option<&'static _> = match (
        cli.config.as_deref(),
        read_initial_toml(cli.config.as_deref()),
    ) {
        (Some(path), Some(snapshot)) => {
            match molesignal::config::watcher::spawn_config_watcher(
                path.to_path_buf(),
                snapshot,
                |diffs| {
                    for d in diffs {
                        if d.immutable {
                            tracing::warn!(field = %d.path, "config change ignored (immutable, restart required)");
                        } else {
                            tracing::info!(field = %d.path, "config changed (hot-apply not yet wired)");
                        }
                    }
                },
            ) {
                Ok(w) => Some(Box::leak(Box::new(w))),
                Err(e) => {
                    tracing::warn!(error = %e, "config watcher disabled");
                    None
                }
            }
        }
        _ => None,
    };

    // JWT secret 不再从 config 读，由 DB 持久化。
    // 仅打 deprecation 警告（若旧 [auth].jwt_secret 字段仍在）。
    settings.auth.warn_if_deprecated_secret_present();

    // 离线子命令：KEK 轮换重包。跑完即退出，不起服务。
    if let Some(Command::RewrapKek { old_key }) = &cli.command {
        tracing::info!("kek rewrap starting (offline)");
        let report = rewrap_kek(settings, old_key).await?;
        for (table, rows) in &report {
            tracing::info!(table = %table, rows = *rows, "kek rewrap");
            println!("rewrapped {rows} rows in {table}");
        }
        tracing::info!("kek rewrap done; verify counts before retiring the old KEK");
        return Ok(());
    }

    tracing::info!(
        roles = ?settings.node.roles,
        bind = %settings.http.bind,
        port = settings.http.port,
        "molesignal starting"
    );

    let mut app_state = build_state(settings).await?;
    if let Some(hub) = logger_guard.self_telemetry() {
        activate_self_telemetry(&mut app_state, settings, hub)?;
    }

    // 按生效角色集起去重后的前台 server（后台 loop 已在 build_state 按角色 spawn）。
    // 任一 server 退出/出错即整体结束（fail-loud）。
    roles::run(app_state, settings).await
}

fn parse_log_format(s: &str) -> molesignal::shared::telemetry::LogFormat {
    use molesignal::shared::telemetry::LogFormat;
    match s {
        "json" => LogFormat::Json,
        _ => LogFormat::Text,
    }
}

fn build_log_output(
    t: &molesignal::config::TelemetrySettings,
) -> molesignal::shared::telemetry::LogOutput {
    use molesignal::shared::telemetry::{FileRotation, LogOutput};
    match t.log_output.as_str() {
        "file" => LogOutput::File {
            directory: t.log_directory.clone().unwrap_or_else(|| "logs".into()),
            file_name_prefix: t
                .log_file_prefix
                .clone()
                .unwrap_or_else(|| "molesignal".into()),
            rotation: match t.log_rotation.as_deref() {
                Some("minutely") => FileRotation::Minutely,
                Some("hourly") => FileRotation::Hourly,
                Some("never") => FileRotation::Never,
                _ => FileRotation::Daily,
            },
            max_log_files: t.log_max_files.unwrap_or(7),
        },
        _ => LogOutput::Console,
    }
}

fn role_str(r: &molesignal::config::Role) -> &'static str {
    use molesignal::config::Role::*;
    match r {
        Standalone => "standalone",
        Router => "router",
        Ingester => "ingester",
        Querier => "querier",
        Compactor => "compactor",
        AlertManager => "alert_manager",
    }
}

/// 读初始 TOML 内容供 watcher 当 baseline；失败返 None 让 watcher 静默禁用。
fn read_initial_toml(path: Option<&std::path::Path>) -> Option<toml::Value> {
    let p = path?;
    let bytes = std::fs::read_to_string(p).ok()?;
    toml::from_str(&bytes).ok()
}
