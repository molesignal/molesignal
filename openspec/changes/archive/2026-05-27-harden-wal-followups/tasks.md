## 1. Settings 扩展与 TOML 默认段

- [x] 1.1 在 `crates/config/src/settings.rs` 新增 `WalFlushStrategy { None, EveryWrite, Batch }` 与 `WalSyncLevel { None, Data, All }` 枚举（`#[serde(rename_all = "snake_case")]`）
- [x] 1.2 `WalSettings` 增加字段：`flush_strategy`（默认 `Batch`）、`sync_level`（默认 `Data`）、`batch_max_pending`（默认 `64`）、`batch_max_delay_ms`（默认 `50`）；全部 `#[serde(default = "...")]`
- [x] 1.3 老 `sync_interval_ms` struct 字段移除（与 alias 冲突），`batch_max_delay_ms` 加 `#[serde(alias = "sync_interval_ms")]`；老 TOML key `sync_interval_ms` 仍被接受并映射
- [x] 1.4 单测 `wal_defaults_yield_batch_data_64_50` + `wal_legacy_sync_interval_ms_alias_maps_to_batch_max_delay_ms`
- [x] 1.5 单测 `wal_flush_strategy_none_round_trip`（兼容性回归）
- [x] 1.6 `conf/config.toml [wal]` 段补 4 个新字段示例 + 运维注释（包含"如何回归旧的 never-fsync 行为"+ legacy alias 说明）

## 2. TermSource trait 与 SegmentWal getter

- [x] 2.1 在 `crates/infra/src/segment_wal/types.rs` 新增 `pub trait TermSource: Send + Sync { fn current_term(&self) -> u64; }`
- [x] 2.2 同文件新增 `pub struct StaticTermSource(pub u64)` + 实现 `TermSource`
- [x] 2.3 在 `crates/infra/src/segment_wal/writer.rs::SegmentWal` 加 getter `pub fn current_term(&self) -> u64 { self.current_term }`
- [x] 2.4 在 `crates/infra/src/segment_wal/mod.rs` 导出 `TermSource` 与 `StaticTermSource`
- [x] 2.5 单测 `static_term_source_returns_inner_value`

## 3. WalPool 构造签名 + open_or_create 注入

- [x] 3.1 `crates/infra/src/ingester/wal_pool.rs::WalPool` 结构体加字段 `fsync_policy: FsyncPolicy` 与 `term_source: Arc<dyn TermSource>`
- [x] 3.2 修改 `WalPool::new` 签名为 `(root, segment_size_bytes, fsync_policy, term_source)`，删除 `DEFAULT_TERM` 常量
- [x] 3.3 `open_or_create` 调 `SegmentWal::new(..., self.fsync_policy, self.term_source.current_term())`，删除 `FsyncPolicy::none_default()` 字面值
- [x] 3.4 `append` 路径在 `wal.lock().await` 之后调一次 `self.term_source.current_term()`，与 `guard.current_term()` 比较，不同则 `guard.set_term(new)`
- [x] 3.5 修复 `wal_pool.rs::tests` 3 个 + `sink.rs::tests` 2 个调用（test_pool helper / 内联 none_default + StaticTermSource(1)）
- [x] 3.6 新增单测 `every_write_fsync_round_trip`：`EveryWrite{Data}` 写 1 条后 readonly 扫描 CRC + payload OK
- [x] 3.7 新增单测 `static_term_source_propagates_to_record_header`：`StaticTermSource(7)` 注入后 record.term == 7
- [x] 3.8 新增单测 `term_change_between_appends_reflected_in_record_headers`：AtomicTermSource 在两次 append 间切换 7→9，两条 record header 分别为 7 / 9

## 4. Bootstrap wire 拼装

- [x] 4.1 在 `crates/bootstrap/src/wire.rs` 加 `fn build_fsync_policy(wal: &WalSettings) -> FsyncPolicy` + 3 个单测覆盖三档分支
- [x] 4.2 wire.rs `WalPool::new` 调用：传 `build_fsync_policy(&settings.wal)` + `Arc::new(StaticTermSource(1)) as Arc<dyn TermSource>`
- [x] 4.3 启动期 `tracing::info!("wal fsync policy resolved", ...)`：输出 dir / strategy / sync_level / batch 字段
- [x] 4.4 修复 `it_grpc_ingest.rs` + `it_ingester_flush.rs` 中的 `WalPool::new` 调用（`it_rum_ingest.rs` 未直接构造 WalPool，跳过）

## 5. Per-key 锁观测指标

- [x] 5.1 在 `crates/infra/src/ingester/metrics.rs` 注册 `wal_append_lock_wait_seconds` Histogram + `wal_append_inflight` IntGauge（label `stream_type`，buckets `[0.0001, 0.001, 0.01, 0.1, 1.0]`）
- [x] 5.2 在 `wal_pool.rs::append` 加 `Instant::now()` → `wal.lock().await` → `observe_wal_lock_wait` 埋点
- [x] 5.3 实现 `WalInflightGuard` RAII（`enter(stream_type)` `+1`，`Drop` `-1`），append 临界区入口 hold 一个
- [x] 5.4 单测 `lock_wait_metrics_recorded_under_concurrent_append`：8 并发 append（Enrichment 标签避开其它测试干扰）→ histogram delta ≥ 8，inflight gauge 归 0

## 6. fsync 错误指标

- [x] 6.1 新增 `crates/infra/src/segment_wal/metrics.rs` 模块，注册 `wal_fsync_errors_total{kind}` Counter（kind ∈ `batch_flush / every_write / segment_rotate`），mod.rs 挂载子模块
- [x] 6.2 `SegmentWal::flush_and_fsync` 的 `EveryWrite` / `Batch` 分支 + `drain_pending_batch_sync` 的 sync 调用全部 catch err 后 `inc_fsync_error(...)`，错误继续向上抛出（不改变现有行为）
- [x] 6.3 单测 `fsync_error_counter_increments_per_label`：白盒断言 counter 接线 + label 隔离。原任务的"fail-on-sync FS"做法跨平台困难（macOS 无 /dev/full、各 FS 失败语义不一），改为走读 + 白盒，已记入测试注释

## 7. Spec 更新（archive 阶段自动应用 delta）

- [x] 7.1 spec delta 已在 change 创建时写入（MODIFIED `Requirement: Write-Ahead Log Durability`，含 batch / every_write / none + sync_level 完整三档行为 + sync_interval_ms alias）
- [x] 7.2 ADDED `Requirement: WAL Fsync Policy Honored At Runtime`（含 fsync_errors_total 计数与不重试约定）
- [x] 7.3 ADDED `Requirement: WAL Per-Key Append Observability`（含 cardinality bound 与 inflight 归零 scenario）
- [x] 7.4 ADDED `Requirement: WAL Term Source Injection Seam`（含未来 raft swap 不改 WalPool 签名约定）

## 8. 文档与 release notes

- [x] 8.1 `ARCHITECTURE.md` "WAL 段文件格式" 段后追加 "Fsync 策略与调优" 小节：三档默认值表 + 调优指南 + 三个新 metric + TermSource seam
- [x] 8.2 release notes 草稿 `openspec/changes/harden-wal-followups/release-notes.md`：含 breaking change 提示、新默认值、opt-out 配方、新 metric 表、forward-compat 说明、upgrade checklist

## 9. 验收

- [x] 9.1 `cargo test -p molesignal-config -p molesignal-infra -p molesignal-bootstrap --lib` 全绿（config 12 / bootstrap 13 / infra 152 = 177 passed, 0 failed）
- [x] 9.2 `cargo test -p molesignal-bootstrap --features enterprise --lib` 全绿（20 passed，enterprise 链入未破）
- [ ] 9.3 staging 部署 ≥ 24h，连续 ingest，监控 `wal_append_lock_wait_seconds` p95、`wal_fsync_errors_total`、ingest p99 latency 三条曲线 **（运维侧，本 PR 不阻塞）**
- [ ] 9.4 staging 期间若任一 `stream_type` 的 `wal_append_lock_wait_seconds` p95 > 5ms 持续 30 分钟，开 follow-up issue "WAL per-key mutex bottleneck" **（运维侧，本 PR 不阻塞）**
- [ ] 9.5 staging 期间 `wal_fsync_errors_total` 应为 0；非零即说明磁盘 / fs 异常，立即排查 **（运维侧，本 PR 不阻塞）**
- [ ] 9.6 验收完成后调 `/openspec-archive harden-wal-followups` **（待 9.3-9.5 通过后执行）**
