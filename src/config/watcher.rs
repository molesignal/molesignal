// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Config file 热重载（spec config-watcher）。
//!
//! 模型：
//! - 启动 watcher：`spawn_config_watcher(path, on_change)`；底层 `notify` crate
//!   监听文件 Modify / Create 事件
//! - 触发后：重新解析 TOML → 与旧 Settings diff → 仅 apply 可热重载的字段；
//!   immutable 字段（dsn / pool bounds / wal.dir / http.port / grpc.port / node.id）变化只
//!   warn 不 apply
//!
//! 当前实装：watcher 骨架 + diff API；具体字段 apply 闭包由调用方传（避免本 crate
//! 反过来依赖 app 层）。
//!
//! 简化：不引 macro 来标 `#[hot_reloadable]`；用一组静态字段名清单。

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::shared::{Error, Result};

/// immutable 字段路径列表（点分隔 TOML 路径）。命中这些字段的变化只 warn。
pub const IMMUTABLE_FIELDS: &[&str] = &[
    "store.meta.dsn",
    "store.meta.min_connections",
    "store.meta.max_connections",
    "wal.dir",
    "http.port",
    "grpc.port",
    "node.id",
    // auth.jwt_secret 已从 settings 删除（DB 持久化，auth-hardening）
    // MS_CIPHER_KEY 走 env，不通过 TOML 热重载
];

/// 单字段变化记录。
#[derive(Debug, Clone)]
pub struct FieldDiff {
    pub path: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
    pub immutable: bool,
}

/// 把两个 TOML 值的差异展平为字段路径 → (old, new)。
pub fn diff_toml(old: &toml::Value, new: &toml::Value) -> Vec<FieldDiff> {
    let mut out = Vec::new();
    diff_recurse("", old, new, &mut out);
    out
}

fn diff_recurse(prefix: &str, old: &toml::Value, new: &toml::Value, out: &mut Vec<FieldDiff>) {
    use toml::Value::Table;
    match (old, new) {
        (Table(a), Table(b)) => {
            let mut keys: std::collections::BTreeSet<&str> = a.keys().map(String::as_str).collect();
            for k in b.keys() {
                keys.insert(k);
            }
            for k in keys {
                let next_prefix = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefix}.{k}")
                };
                match (a.get(k), b.get(k)) {
                    (Some(av), Some(bv)) if av != bv => diff_recurse(&next_prefix, av, bv, out),
                    (None, Some(bv)) => {
                        out.push(FieldDiff {
                            path: next_prefix.clone(),
                            old_value: serde_json::Value::Null,
                            new_value: toml_to_json(bv),
                            immutable: IMMUTABLE_FIELDS.contains(&next_prefix.as_str()),
                        });
                    }
                    (Some(av), None) => {
                        out.push(FieldDiff {
                            path: next_prefix.clone(),
                            old_value: toml_to_json(av),
                            new_value: serde_json::Value::Null,
                            immutable: IMMUTABLE_FIELDS.contains(&next_prefix.as_str()),
                        });
                    }
                    _ => {}
                }
            }
        }
        (a, b) if a != b => {
            out.push(FieldDiff {
                path: prefix.to_string(),
                old_value: toml_to_json(a),
                new_value: toml_to_json(b),
                immutable: IMMUTABLE_FIELDS.contains(&prefix),
            });
        }
        _ => {}
    }
}

fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

/// 启动 config 文件 watcher。回调 `on_change` 在收到变化事件 + diff 计算完后调用。
/// 返回 `Arc<Watcher>` 保活；drop 时 watcher 自动停止。
///
/// `initial_snapshot` 是 bootstrap 阶段已经成功 parse 的 TOML 内容；watcher 内部维护
/// `last_snapshot`，每次文件变化 reload 并与之 diff。
pub fn spawn_config_watcher<F>(
    path: PathBuf,
    initial_snapshot: toml::Value,
    on_change: F,
) -> Result<Arc<notify::RecommendedWatcher>>
where
    F: Fn(Vec<FieldDiff>) + Send + Sync + 'static,
{
    use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    let last_snapshot = Arc::new(parking_lot::Mutex::new(initial_snapshot));
    let path_for_callback = path.clone();
    let snapshot_for_callback = last_snapshot.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "config watcher event error");
                    return;
                }
            };
            if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                return;
            }
            let diffs = match reload_and_diff(&path_for_callback, &snapshot_for_callback) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(error = %e, "config reload failed");
                    return;
                }
            };
            if diffs.is_empty() {
                return;
            }
            // 标记 immutable 字段
            for d in &diffs {
                if d.immutable {
                    tracing::warn!(
                        field = %d.path,
                        "immutable config field changed; restart required to apply"
                    );
                }
            }
            on_change(diffs);
        },
        Config::default().with_poll_interval(Duration::from_secs(2)),
    )
    .map_err(|e| Error::internal(format!("config watcher: {e}")))?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| Error::internal(format!("config watch: {e}")))?;
    Ok(Arc::new(watcher))
}

/// 读盘 → parse → 与 last_snapshot diff → 把 last_snapshot 更新为最新值。
///
/// 失败（IO / parse 错）保持 last_snapshot 不动，返回 Err 让调用方决定 log。
fn reload_and_diff(
    path: &Path,
    last_snapshot: &parking_lot::Mutex<toml::Value>,
) -> Result<Vec<FieldDiff>> {
    let bytes =
        std::fs::read_to_string(path).map_err(|e| Error::internal(format!("config read: {e}")))?;
    let new: toml::Value =
        toml::from_str(&bytes).map_err(|e| Error::invalid(format!("config parse: {e}")))?;
    let mut guard = last_snapshot.lock();
    let diffs = diff_toml(&guard, &new);
    *guard = new;
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> toml::Value {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn diff_finds_changed_scalar() {
        let old = parse(
            r#"[telemetry]
log_level = "info""#,
        );
        let new = parse(
            r#"[telemetry]
log_level = "debug""#,
        );
        let diffs = diff_toml(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "telemetry.log_level");
        assert!(!diffs[0].immutable);
    }

    #[test]
    fn diff_marks_immutable_meta_dsn() {
        // auth-hardening：原本 auth.jwt_secret 被列为 immutable，现已从
        // Settings 删除（DB 持久化）；改用 store.meta.dsn 校验 immutable 行为。
        let old = parse(
            r#"[store.meta]
dsn = "postgres://a""#,
        );
        let new = parse(
            r#"[store.meta]
dsn = "postgres://b""#,
        );
        let diffs = diff_toml(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "store.meta.dsn");
        assert!(diffs[0].immutable);
    }

    #[test]
    fn diff_marks_pool_bounds_immutable() {
        let old = parse(
            r#"[store.meta]
min_connections = 1
max_connections = 8"#,
        );
        let new = parse(
            r#"[store.meta]
min_connections = 2
max_connections = 16"#,
        );
        let diffs = diff_toml(&old, &new);

        assert_eq!(diffs.len(), 2);
        assert!(diffs.iter().all(|diff| diff.immutable));
    }

    #[test]
    fn diff_handles_added_and_removed_keys() {
        let old = parse(
            r#"[a]
x = 1"#,
        );
        let new = parse(
            r#"[a]
y = 2"#,
        );
        let diffs = diff_toml(&old, &new);
        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"a.x"));
        assert!(paths.contains(&"a.y"));
    }

    #[test]
    fn identical_tomls_no_diff() {
        let v = parse(
            r#"[x]
a = 1"#,
        );
        assert!(diff_toml(&v, &v).is_empty());
    }
}
