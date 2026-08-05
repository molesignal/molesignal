## Context

`crates/infra/src/segment_wal/` 已经实现了 production-grade 的 segment-based WAL：

- 32B 头 + CRC32C（覆盖 head[0..28] + payload）
- 三档 `FsyncPolicy { None / EveryWrite / Batch }` × 三档 `SyncLevel { NONE / DATA / ALL }`
- `sync_dir_parent_of` 处理 Linux rename 后目录项 durability
- mmap 读 + tail-corruption 自动截尾
- raft-friendly header（`term: u64 / index: u64`）

但 `WalPool` 这一层（`crates/infra/src/ingester/wal_pool.rs`）作为 ingester role 与 `SegmentWal` 之间的"按 stream 分桶 + dashmap 复用 + 启动 recover"封装，写死了两个值：

```rust
// wal_pool.rs:107
FsyncPolicy::none_default(),

// wal_pool.rs:30 / wal_pool.rs:108
const DEFAULT_TERM: u64 = 1;
```

并且对临界区 `wal.lock().await`（wal_pool.rs:119）的等待时间没有任何观测。spec 里承诺的 fsync 行为没有兑现；未来 raft 接入需要重构构造签名；mutex 是否成为热 stream 瓶颈没有数据。

OpenObserve 对应路径（`src/wal/src/`）在 1.x 也走过同样阶段：先有 segment + CRC + fsync 三档，然后才把 fsync policy 接进 config，再后来挂 raft。本 change 不实现 raft，但把 seam 留好。

## Goals / Non-Goals

**Goals:**

- `WalSettings` 暴露完整 `FsyncPolicy` 选择，老 TOML 零修改启动。
- 默认从"never fsync"升级为"50ms / 64 条 batch fsync（sync_data）"，与 spec 文本对齐。
- 运维可在 `/metrics` 直接观察"哪个 stream_type 在 WAL 锁上等多久"。
- `WalPool` 提供 `Arc<dyn TermSource>` 注入点，raft 接入只需 swap 实现。
- 不破坏既有 8 个 `wal_pool` / `ingester` 单测。

**Non-Goals:**

- 不实装 raft 共识层（只留 seam）。
- 不重写 per-key 并发模型（只加观测，下一阶段决策）。
- 不引入第二种 WAL 后端（RocksDB / fjall / sled 等），延续 segment WAL。
- 不改 WAL header / 记录格式 / `WAL_VERSION`，仍是 v2。
- 不改 ingester 入口 `IngesterSink::write` 的 wal-before-buffer 顺序。
- 不引入 fsync error retry / 降级策略（fsync 错继续往上抛 + 记 metric）。

## Decisions

### 1. `WalSettings` 字段命名与默认值

| 字段 | 类型 | 默认 | 备注 |
|---|---|---|---|
| `dir` | `String` | `./data/wal` | 已有 |
| `segment_size_mb` | `u32` | `256` | 已有 |
| `flush_strategy` | `WalFlushStrategy` (`"none" / "every_write" / "batch"`) | `"batch"` | **新** |
| `sync_level` | `WalSyncLevel` (`"none" / "data" / "all"`) | `"data"` | **新** |
| `batch_max_pending` | `u32` | `64` | **新**，仅 `batch` 生效 |
| `batch_max_delay_ms` | `u32` | `50` | **新**，仅 `batch` 生效 |
| `sync_interval_ms` | `u32` | `50` | **保留**，serde alias of `batch_max_delay_ms`（先认 `batch_max_delay_ms`，缺则 fallback `sync_interval_ms`） |

枚举走 `#[serde(rename_all = "snake_case")]`，TOML 字符串字面值方便运维心智。

**替代方案 A**：直接复用 `FsyncPolicy` enum + serde tagged。否决：`FsyncPolicy::Batch { ... }` 的嵌套字段写在 TOML 里是 nested table，跟 `[wal]` 段平铺风格不一致。

**替代方案 B**：把 `sync_interval_ms` 直接重命名为 `batch_max_delay_ms`。否决：破坏向后兼容。

### 2. 默认值理由：`Batch { 64, 50ms, Data }`

- `None`：等价于今天的行为（page cache only），不能作 default 默认上线，否则 spec bug 留着没修。
- `EveryWrite`：每条记录都 fsync，单条 ingest 延迟从 ~10µs 飙到 ~ms（取决于磁盘），p99 不可接受。
- `Batch { max_pending=64, max_delay_ms=50, SyncLevel::Data }`：
  - 50ms = 现有 `default_wal_sync()` 值，向后语义对齐
  - 64 条 = 单 segment 256 MiB / typical batch payload（≤ 256 KiB JSON）下，64 条≈ 16 MiB，远未触发 segment rotate；上限是为了防长尾延迟，避免一直攒不够导致 fsync 永远不触发
  - `Data`（`sync_data`）= 数据落盘但不强制 metadata，是大多数 LSM/WAL 的默认（比 `All` 快 ~30%，比 `None` 安全得多）
- 配套：父目录 fsync 仅在 `sync_level = All` 时调用（types.rs:109-121 已实现）—— 默认 `Data` 下不付这层代价

### 3. `TermSource` trait 边界

```rust
// crates/infra/src/segment_wal/types.rs
pub trait TermSource: Send + Sync {
    fn current_term(&self) -> u64;
}

pub struct StaticTermSource(pub u64);
impl TermSource for StaticTermSource {
    fn current_term(&self) -> u64 { self.0 }
}
```

- `Arc<dyn TermSource>` 比 `fn() -> u64` 灵活：raft 实现可以持 `Arc<RaftNode>` 内部状态
- `current_term(&self)` 用 `&self` 而非 `&mut self`：raft 内部用 `AtomicU64::load` 即可，免锁
- **不**让 `TermSource` 决定 `index`：index 是 WAL 内部的 sequence（IngesterSink::next_seq），跟 consensus index 是两个概念；raft 接入再判断是否要换为 raft log index（可能拆 follow-up）
- `WalPool::append` 在拿 `wal.lock().await` 之后、`write_raw` 之前调一次 `term_source.current_term()`，与 `wal.current_term` 比较，不同则 `wal.set_term(new)`（writer.rs:73 已有方法）。开销：单次 atomic load + 比较，可忽略

### 4. Per-key mutex 观测的 cardinality 控制

label 选择经过权衡：

| 候选 label 集 | cardinality | 选/否 |
|---|---|---|
| `stream_type` | 4 (logs/metrics/traces/enrichment) | ✅ 选 |
| `stream_type + org_id` | 4 × org 数 | ❌ org 数无上限 |
| `stream_type + stream_name` | 4 × stream 数 | ❌ stream 数无上限 |
| 无 label | 1 | ⚠️ 太粗，看不出 logs vs metrics 谁慢 |

Histogram buckets：`[100µs, 1ms, 10ms, 100ms, 1s]`（5 段，常规 WAL 锁等待在 µs 级，>10ms 已经是异常）。

**触发后续优化的阈值**：staging 跑 ≥ 24h，若某 `stream_type` 的 p95 `wal_append_lock_wait_seconds > 5ms`，立项 follow-up（候选方案：sharded WAL by `hash(stream) % N`、per-segment double-buffer、async batch append）。该阈值写进本 change tasks.md 的验收项。

### 5. `WalPool::new` 签名破坏性变更的影响范围

```bash
$ grep -rn "WalPool::new\|WalPool::new(" crates/ --include="*.rs"
```

主代码 callsite：
- `crates/bootstrap/src/wire.rs:196`（单一生产 callsite）

测试 callsite：
- `crates/infra/src/ingester/wal_pool.rs::tests`（3 个）
- `crates/infra/src/ingester/sink.rs::tests`（2 个）
- `crates/bootstrap/tests/it_ingester_flush.rs`、`it_grpc_ingest.rs`、`it_rum_ingest.rs`（少量）

数量小、定位明确，签名扩两个参数全部一次性 fix。**不**做 builder 包装、不引 `WalPoolBuilder`：当前 callsite 远不到需要 builder 的规模，硬加 builder 是 over-engineering。

### 6. fsync 错误处理策略

`SegmentWal::write_record` 内部已经处理 fsync 失败（return Err）。`WalPool::append` 把 error 往上抛到 `IngesterSink::write`（sink.rs:80-83），现有路径就是 `Error::internal`。本 change 增加：

- `wal_fsync_errors_total{kind}`（Counter，label `kind ∈ {batch_flush, every_write, segment_rotate}`），便于区分哪种触发模式失败

不引入 retry / fallback：fsync 失败是设备级问题，重试无意义；上层 ingester 已经会把错误返给 client（HTTP 500），client 自带 retry。

### 7. 老配置文件兼容性

- 老 TOML 写 `sync_interval_ms = 50` 不带 `flush_strategy` / `sync_level`：`#[serde(default)]` 让缺字段拿默认值（`Batch / Data / 64 / 50ms`，其中 50ms 来自 `sync_interval_ms` 的 alias）→ 行为：从"never fsync"变为"50ms batch fsync"。**这是真实行为变化**，release notes 必须显式声明。
- 老 TOML 显式希望保留旧行为：写 `flush_strategy = "none"`，等价 `FsyncPolicy::none_default()`。

不引"silent backwards-compat" —— 如果 spec 一直承诺 fsync 而实现没做，本次修复就是兑现承诺，不应为兼容老的错误行为而留 default = `"none"`。

### 8. observability 接入位置

```rust
// wal_pool.rs::append（修改后伪代码）
pub async fn append(&self, key: &WalKey, payload: &[u8], seq: u64) -> Result<()> {
    let wal = self.open_or_create(key)?;
    let lock_started = Instant::now();
    let mut guard = wal.lock().await;
    let wait = lock_started.elapsed();
    let stream_type = stream_type_str(key.1);
    wal_append_lock_wait_seconds().with_label_values(&[stream_type]).observe(wait.as_secs_f64());
    let _inflight = WalInflightGuard::enter(stream_type); // RAII +1 / -1 on drop

    let term = self.term_source.current_term();
    if guard.current_term() != term {
        guard.set_term(term);
    }
    guard.write_raw(WalEntryType::Normal, payload, seq)?;
    Ok(())
}
```

`SegmentWal::current_term` 是新增的 getter（writer.rs 加 `pub fn current_term(&self) -> u64 { self.current_term }`），免得 `WalPool` 自己再维护一份 term。

### 9. release notes 草稿口径

> **Breaking durability behavior change.** WAL fsync now follows `[wal].flush_strategy / sync_level / batch_*` (default `Batch { max_pending=64, max_delay_ms=50, sync_level=data }`). Prior versions hardcoded "never fsync (page cache only)" despite the spec promising otherwise. To restore the legacy behavior, set `[wal].flush_strategy = "none"`. Expected p99 ingest latency impact: < 5%; crash durability significantly improved.

## Open Questions（带入实施阶段决定，不阻塞 propose）

- `batch_max_pending` 是按 segment-local 计数还是 cross-segment 累计？现有 `SegmentWal.pending_sync: usize` 已经是 segment-local（segment rotate 时归零），保留该语义，文档化即可。
- `wal_append_lock_wait_seconds` 是否要加 `result="ok|err"` 维度？倾向于 No —— 等待时间和错误无关，等待完后才知道结果，加 label 反而难解释。错误统计已有 `wal_fsync_errors_total`。
- `StaticTermSource(1)` 的初值 `1` 是否要从 settings 读？倾向于 No —— 单机非共识场景下 term 是哑值，硬编码 `1` 与 `DEFAULT_TERM` 一致；接 raft 时由 raft 状态机决定，settings 不掺和。

## Validation

- `cargo test -p molesignal-config -p molesignal-infra -p molesignal-bootstrap` 全绿
- 新增单测：
  - `WalSettings` 反序列化（缺字段 default、`sync_interval_ms` alias 生效、`flush_strategy = "none"` 显式回归）
  - `WalPool` fsync policy 透传（构造时传 `Batch`，写若干条后从磁盘 mmap 读回 verify）
  - `WalPool` term source 注入（`StaticTermSource(7)` 写一条 → mmap 读 header 验 term=7）
  - `wal_append_lock_wait_seconds` 在并发 append 下被记录（label = `logs`）
- staging 跑 ≥ 24h，确认 `wal_append_lock_wait_seconds` p95 < 5ms（如超过即激活 follow-up 立项条件）
- staging 跑过程中 `wal_fsync_errors_total = 0`（一旦非零说明磁盘 / 配置异常）
