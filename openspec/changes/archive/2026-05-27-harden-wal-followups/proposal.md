## Why

WAL 层 spec 与实现已经分叉，对应三个 follow-up：

1. **fsync 配置没接进来（durability bug）**。`openspec/specs/ingestion/spec.md` 的 `Requirement: Write-Ahead Log Durability` 明文写 "fsync throttled to `wal.sync_interval_ms`"，但 `crates/infra/src/ingester/wal_pool.rs:107` 实际是：
   ```rust
   FsyncPolicy::none_default(),  // = FsyncPolicy::None { sync_level: SyncLevel::NONE }
   ```
   即"flush 进 page cache，不调 `sync_*`"。`wal.sync_interval_ms = 50` 这个配置项在代码里**根本没有读取点**。crash 后内核 panic / power loss 会丢"flush 已成功但 fsync 未触发"窗口内的 ingest batch。`SegmentWal` 内部已经有完整的 `FsyncPolicy { None / EveryWrite / Batch { max_pending, max_delay_ms, sync_level } }` + `SyncLevel { NONE / DATA / ALL }` 实现（`crates/infra/src/segment_wal/types.rs:32-184`）和 `sync_dir_parent_of`（types.rs:109-121），只是配置层没有暴露选择项 / wire 没接。

2. **per-key mutex 在高并发下吞吐未量化**。`WalPool::open_or_create` 用 `Arc<Mutex<SegmentWal>>` per `(org, stream_type, stream)`（wal_pool.rs:72-114），同 stream 并发 ingest 全部串到一把 tokio mutex。这是 append-only 顺序性的必要约束（同 segment 不能并发写），但目前没有 `wal_append_lock_wait` / `wal_append_inflight` 之类的指标，运维无法在生产里观测某个热门 stream 是否被该锁拖到队列堆积。在确定是否需要重构（如 sharded WAL / per-segment double-buffer）之前，先把观测加上。

3. **`DEFAULT_TERM = 1` 写死，raft 无法注入**。WAL record header 已经预留 `term(8B) | index(8B)` 字段（`crates/infra/src/segment_wal/types.rs:188-193`），`SegmentWal` 也已经暴露 `set_term(term: u64)`（writer.rs:73-75），但 `WalPool::open_or_create` 调 `SegmentWal::new(..., DEFAULT_TERM)`（wal_pool.rs:108）后没有任何路径能在 runtime 把 term 推进去。未来接 raft 时，consensus 层必须能在 leader election 后告知 WAL "当前 term 是 N"，不应该回头去改 `WalPool` 的构造签名。提前把"term 由谁提供"抽成 trait，raft 接入变成实现一个新 `TermSource`，零侵入。

三条都没有破坏性改动，但项 1 是已经在 spec 里承诺、却没兑现的实现 bug；项 2、3 是为后续工作（raft + 性能调优）打地基的最小 seam。

## What Changes

### 1. `WalSettings` 扩展 fsync policy（向后兼容）

`crates/config/src/settings.rs::WalSettings` 新增：
- `flush_strategy: WalFlushStrategy { None, EveryWrite, Batch }`（默认 `Batch`）
- `sync_level: WalSyncLevel { None, Data, All }`（默认 `Data` = `sync_data(2)`）
- `batch_max_pending: u32`（默认 `64`）
- `batch_max_delay_ms: u32`（**复用** 现有 `sync_interval_ms` 字段作为 alias，TOML 写 `sync_interval_ms` 仍可被 `batch_max_delay_ms` 接收）

老 `sync_interval_ms: u32` 字段保留 + `#[serde(alias = "batch_max_delay_ms")]`，老 TOML 零修改启动。

### 2. `WalPool` 接受 fsync policy 与 term source 注入

```rust
pub struct WalPool {
    root: PathBuf,
    segment_size_bytes: usize,
    fsync_policy: FsyncPolicy,         // 新增
    term_source: Arc<dyn TermSource>,  // 新增
    pools: DashMap<WalKey, Arc<Mutex<SegmentWal>>>,
}

impl WalPool {
    pub fn new(
        root: impl Into<PathBuf>,
        segment_size_bytes: usize,
        fsync_policy: FsyncPolicy,            // 新增
        term_source: Arc<dyn TermSource>,     // 新增
    ) -> Self { ... }
}
```

`open_or_create` 把字段透给 `SegmentWal::new`，**不再调 `FsyncPolicy::none_default()`**。

### 3. `TermSource` trait

在 `crates/infra/src/segment_wal/types.rs` 新增：

```rust
pub trait TermSource: Send + Sync {
    fn current_term(&self) -> u64;
}

pub struct StaticTermSource(pub u64);
impl TermSource for StaticTermSource {
    fn current_term(&self) -> u64 { self.0 }
}
```

`WalPool::append`（以及 truncate 不需要）在写入前若 `term_source.current_term() != wal.current_term()` 则调 `SegmentWal::set_term(...)`；新创建的 wal 用 `term_source.current_term()` 作为 `initial_term`，**不再写死 `DEFAULT_TERM = 1`**。

### 4. Bootstrap wire 拼装

`crates/bootstrap/src/wire.rs:196`：

```rust
// before:
let wal_pool = Arc::new(WalPool::new(&settings.wal.dir, segment_bytes));

// after:
let fsync_policy = build_fsync_policy(&settings.wal);
let term_source: Arc<dyn TermSource> = Arc::new(StaticTermSource(1));
let wal_pool = Arc::new(WalPool::new(
    &settings.wal.dir,
    segment_bytes,
    fsync_policy,
    term_source,
));
```

`build_fsync_policy` 把 settings 三档字段映射到 `FsyncPolicy` 枚举。

### 5. Per-key 观测指标

新增两个 Prometheus 序列：

- `wal_append_lock_wait_seconds`（Histogram，label `stream_type`；buckets `[0.0001, 0.001, 0.01, 0.1, 1.0]`）—— `WalPool::append` 在 `wal.lock().await` 前后量等待时间
- `wal_append_inflight`（IntGauge，label `stream_type`）—— 进入临界区时 +1、离开时 -1

**label 控制**：只用 `stream_type`（4 个值），**不**带 `stream_name` 或 `org_id`，避免 high cardinality 爆。

### 6. Spec 更新

`openspec/specs/ingestion/spec.md`：
- MODIFIED `Requirement: Write-Ahead Log Durability`：明确支持三档 + sync_level + batch 触发条件，并要求 runtime 必须 honor 配置
- ADDED `Requirement: WAL Fsync Policy Honored At Runtime`
- ADDED `Requirement: WAL Per-Key Append Observability`
- ADDED `Requirement: WAL Term Source Injection Seam`

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `ingestion`：增强 WAL durability requirement，新增 3 条 Requirement（fsync runtime honored / per-key observability / term source seam）。

## Impact

- **行为变化（重要）**：默认 `flush_strategy = Batch` + `sync_level = Data`，从"完全不 fsync（仅 page cache）"升级为"50ms 或 64 条触发一次 `sync_data`"。这是 durability 的实质增强，release notes 必须标注。性能影响：单条 ingest 路径不阻塞 fsync（fsync 在 batch 间隔触发，与 write 解耦），p99 ingest 延迟预期影响 < 5%；fsync 失败的暴露面变大（之前永远不报 fsync 错），新增 `wal_fsync_errors_total{step}` 指标兜底。
- **配置**：`conf/config.toml [wal]` 段增加 3 个字段，全部带 default，老 TOML 兼容。
- **代码改动**：
  - `crates/config/src/settings.rs`：`WalSettings` 扩展
  - `crates/infra/src/segment_wal/types.rs`：新 `TermSource` trait + `StaticTermSource`
  - `crates/infra/src/ingester/wal_pool.rs`：构造签名 + `open_or_create` policy/term 注入 + append 路径指标埋点
  - `crates/bootstrap/src/wire.rs`：`build_fsync_policy` + `Arc<StaticTermSource(1)>` 注入
  - `conf/config.toml`：`[wal]` 段新字段示例 + 注释
- **依赖**：不引新 crate。
- **测试**：新增 settings 反序列化 default 单测、`WalPool` fsync policy 透传单测、term_source set/update 单测；保留已有 `append_and_recover_round_trip` 等覆盖。
- **未来 raft 接入**：实现 `RaftTermSource { fn current_term(&self) -> u64 { self.raft.current_term() } }` 并在 wire 阶段 swap，**不需要再动 WalPool 签名**。
- **per-key mutex 重构（Non-goal）**：本 change 只加观测，不重写并发模型。等 staging 数据回来再单独 propose（条件：p95 `wal_append_lock_wait_seconds` 在热 stream 持续 > 5ms 即触发立项）。
