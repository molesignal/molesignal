## Context

`implement-backend-mvp`（已 archive）的现状：
- `wire::build_state` 已经把 14 张表的 repository + service 装好，HTTP/`/api/v1/*` 全链路可走（login → ingest → query → dashboard import）。
- 真实 IO 落地：`SegmentWal`（32B header + crc32c + lz4 + fsync 策略）、`ParquetWriter / ParquetReader`、`DataFusionEngine`（朴素 MemTable 路径）、`MultiNotifier`（Slack/Webhook/Email）、`EscalationDispatcher` 派发函数 —— 都过单测/集成测。
- stub 部分：`ingester` 没有 WAL writer / buffer / flush 三件套，`/api/v1/ingest/*` 落点是 `MemoryIngestSink`（数据进程一挂全丢）；`querier / compactor / alert_manager / router` 四个 role 是空 ticker；`PromqlEngine` 是 `UnimplementedPromqlEngine`；`Tantivy` 是空 struct；`/metrics` + OTLP exporter 还没装；gRPC server 装配是 `server_stub()`；identity CRUD 路由没接。

OpenObserve 的核心竞争力（来自项目 README + ARCHITECTURE 关键词）：
- Parquet + Object Store 列存：相比 ES 显著降本，几十倍压缩 + 列裁剪。
- 分区（time bucket）+ 索引（Tantivy FTS + min/max）+ 智能缓存（meta cache + result cache）联合裁剪，搜索空间最多减 99%。
- SQL（DataFusion）+ PromQL 双引擎。
- 原生多租户：`org_id` 与 `stream` 一等公民。
- 无状态架构：worker 进程随起随用，唯一本地状态是 WAL 段（且崩溃可重放）。

约束：
- 同一二进制按 `[node].roles` 启不同 role，role 间共享 `AppState`（standalone）或通过 gRPC 解耦（分布式）。
- 数据库是 PostgreSQL via sqlx + 原生 SQL（不用 ORM）。
- 现有 domain trait 必须复用，repository 已是 trait + impl 两层。
- Rust 1.90 工具链，DataFusion 与 Arrow 主版本已锁。

## Goals / Non-Goals

**Goals:**
- 让 `ingester / querier / compactor / alert_manager / router` 五个 role 从 stub 升级到真实可生产工作负载（仍 MVP 级，非性能调优终局）。
- 让查询前的"分区 + 索引 + 缓存"三件套联合裁剪 `ParquetFileMeta` 候选集 → `ParquetExec` 输入 → 行；目标对常见查询把扫描 file 数压到 1-3 个。
- 让 PromQL 跑通最常见 6 个函数（rate / increase / sum / avg / min / histogram_quantile）+ instant/range query。
- 让多租户在 service / repository / planner 三层一致用 `org_id` 防越权；handler 之外多一层兜底。
- 让 worker 无状态：杀进程 → 启进程 = 仅丢失最近 < flush 窗的未持久化数据（且 WAL 段还在）。

**Non-Goals:**
- DataFusion 自定义 `TableProvider` 的高级谓词下推（仅依赖 `ParquetExec` 默认能力）。
- Bloom filter 持久化（min/max + Tantivy 已足够本期）。
- 跨 region active-active；跨 AZ 数据复制（依赖 object store 自身耐久性）。
- 采样/降采样、AI 异常检测。
- 任何前端改动。
- 完整 PromQL 函数集合（其余函数返 `Error::Invalid("promql function not yet supported: <name>")`）。

## Decisions

### 1. Ingester role：WAL → 内存 Arrow buffer → 周期 flush

**选择**：在 `crates/bootstrap/src/roles/ingester.rs` 启 `IngesterWorker { wal_pool, buffer_pool, flush_scheduler }`。

```
HTTP/gRPC → IngestService::ingest(batch)
            ├─ infer_schema_extension → streams.update_schema
            ├─ wal_pool[(org, stream_type, stream)].append(encoded_batch)  // payload = bincode(IngestBatch)
            └─ buffer_pool[(org, stream_type, stream)].push(record_batch)  // Arrow RecordBatchBuilder

FlushScheduler (tokio::interval):
  for each (key, buffer) where buffer.size >= buffer_max_mb OR age >= flush_interval_secs:
    record_batch = buffer.finish_and_clear()
    parquet_file_meta = ParquetWriter::flush(stream_def, record_batch)
    parquet_file_meta_repo.insert(parquet_file_meta)
    wal_pool[key].truncate_up_to(buffer.wal_high_watermark)
```

**关键点**：
- `WalPool` per `(org, stream_type, stream)` 持有一个 `SegmentWal`；entry 现含 `(seq_index, batch)`，buffer 记录 high-watermark 用于成功 flush 后截断旧段。
- Buffer 用 Arrow `StructArray` 风格的逐字段 builder（`StringBuilder / Int64Builder / Float64Builder / TimestampMicrosecondBuilder`），按 `StreamDefinition.schema` 顺序排列，新字段从 schema_extension 同步过来时追加 null 列填补历史 batch。
- `flush_scheduler` 用一个 `tokio::sync::Notify` + interval；同一 buffer 不并发 flush（buffer 内 mutex）。
- Tantivy 构建并入 `ParquetWriter::flush_with_index(stream, record_batch) → (ParquetFileMeta, TantivyArchive)`，返回的 archive 在 buffer flush 同一 await 点也 put 到 object store。
- 启动时 `IngesterWorker::recover()` 扫所有 WAL 目录，找出最大 `(seq_index)` 没有对应 `ParquetFileMeta` 的段，把里面 batch 全部 replay 回 buffer，等于一次正常的写入路径，然后调一次 force flush。

**替代方案**：直接每条 batch 立即写 parquet（不 buffer）—— 否决：小 parquet 文件爆炸，compactor 跟不上。

### 2. Compactor role：合并 + retention

**选择**：`CompactorWorker::tick` 每 `compactor.interval_secs`：

```
for each (org, stream, stream_type, date):
  candidates = parquet_file_meta_repo.list_small(org, stream, stream_type, date, target_mb)
                 .sort_by(time_start)
  while candidates.len() >= 2:
    group = greedy_take(candidates, target_mb)            // 累计 size 不超 target
    merged_batch = arrow::concat(group.map(parquet_reader.read_all))
    new_meta = parquet_writer.flush(stream_def, merged_batch)
    parquet_file_meta_repo.replace(group.ids, vec![new_meta])     // 事务 5.5
    object_store.delete_each(group.object_keys).await     // 失败只记 warn，下轮 retention 兜底
```

**Retention**：

```
for each stream:
  cutoff = now - stream.retention_secs
  for file in parquet_file_meta_repo.list_older(stream.id, cutoff):
    parquet_file_meta_repo.mark_deleted(file.id)   // 已索引化的 deleted=true 列
    object_store.delete(file.object_key)
```

`deleted = true` 是 tombstone，下次 compaction 直接跳过；后续可加 hard-delete sweep（本期不做）。

**替代方案**：trigger-based compaction（小文件计数过阈值再触发）—— 否决：调度复杂，周期扫足够。

### 3. Distributed Querier：Arrow Flight + 一致性哈希分片

**选择**：每个 querier 同时是 server（`FlightService { do_get }`）也是 client。请求路径：

```
HTTP /api/v1/query (coordinator querier)
  → QueryService::run
  → plan_partitions: parquet_file_meta_repo.find(time_range) → Vec<ParquetFileMeta>
  → 若集群只有自己 → 本地 DataFusionEngine.execute_local(stmt, candidates)
  → 否则:
       peers = cluster_registry.list_role(NodeRole::Querier)
       shards = consistent_hash(parquet_file_meta.object_key) % peers.len()
       futures = peers.zip(shards).map(|(peer, files)|
         FlightClient(peer).do_get(Ticket{stmt, files, projection}))
       merged = arrow_flight::stream_merge(futures)
       result = DataFusion.final_aggregate(merged, stmt)
```

**Ticket** 用 prost 编码：`QueryShard { sql: String, parquet_file_metas: Vec<ParquetFileMeta>, projection: Vec<String>, time_range }`，避免 SQL 二次解析的歧义。

**Server 端 `do_get`**：把 `parquet_file_metas` 喂给一个本地 `DataFusion` ctx（没有 final aggregation —— 只做 scan + filter + projection + group-by partial），输出 `RecordBatch` 流到 `FlightDataStream`。

**为什么不让每个 peer 跑同一条 SQL**：避免 group-by 在每个 peer 都做全局聚合然后还要 final merge 时类型不匹配 —— 让 coordinator 显式拆 partial / final。简化做法：peer 端只做 scan + projection + WHERE，coordinator 端把所有 `RecordBatch` UNION ALL 后跑完整 SQL —— 本期采用这条路径（代价是 final SQL 计算量集中在 coordinator，可接受）。

**单节点 fallback**：当 `cluster_registry.list_role(Querier).len() <= 1`，直接走 `DataFusionEngine` 现有路径，不进 Flight。

**替代方案**：用 DataFusion 内置 `PartitionStream` + 自定义 `TableProvider`（FlightTableProvider）—— 否决：实现复杂度高，本期人工拼 partial/final 足够。

### 4. PromQL 引擎

**选择**：`PromQLEngine { promql_parser, data_fusion: Arc<SessionContext> }`。

```
parse expr → ast
walk ast:
  VectorSelector(metric, labels)
    → scan ParquetFileMeta where stream=metric, time_range overlap query window
    → SQL `SELECT _timestamp, value, labels FROM <metric> WHERE labels_match($matchers) AND _timestamp BETWEEN ..`
    → 得 series Map<label_set, Vec<(ts, value)>>
  Call("rate", VectorSelector{range})
    → 对每个 series 做 windowed: (last - first) / range
  Call("sum", InnerExpr)
    → 按 by/without label 分组求和
  histogram_quantile(q, sum by (le)(rate(metric[5m])))
    → 一次完整流水线 example
```

**Metrics stream 列布局约定**：
- `_timestamp`（micros）
- `value`（f64）
- `labels`（JSONB 实际 PG 列，但 Arrow 端是 `Utf8` 序列化字符串，evaluator 用 `serde_json::Value` 解一次 → 内部哈希）

range query：在 `[start, end]` 上按 `step` 离散，每个采样点跑一次 instant 求值，结果对齐为 `Matrix`。

**未实现的函数**：所有不在 `{rate, increase, sum, avg, min, max, count, histogram_quantile}` 名单内的 PromQL 函数返 `Error::Invalid("promql function not yet supported: <name>")`，让用户清楚边界。

**替代方案**：通过把 PromQL 全量降级翻译成 SQL —— 否决：histogram_quantile 与 rate 的 windowed semantics 翻译 SQL 太脆，自实现 evaluator 更可控。

### 5. Tantivy 倒排索引：同写同存同查

**写**：`ParquetWriter::flush_with_index`：

```
tantivy_index_builder = TantivyIndex::open_in_dir(tempdir)
for record in record_batch:
  for field where stream.schema[field].indexed = true && type = String:
    tantivy_index_builder.add_doc({field_name: value, _row_idx: i})
tantivy_index_builder.commit()
archive = tar_zstd(tempdir)
object_store.put(format!("{object_key}.tantivy.tar.zst"), archive)
```

**查**：planner 阶段抓 `WHERE` 中的 `MATCH(field, term)` 谓词 →

```
for fm in candidates:
  archive = object_store.get(format!("{}.tantivy.tar.zst", fm.object_key))
  index = untar_zstd_into(tempdir).open()
  if index.searcher().query(field, term).count() == 0:
    remove fm from candidates
```

**MATCH 谓词暴露**：DataFusion 已支持 `MATCH(field, 'term')` 作为 user function，本期注册 udf `match` 返 bool；planner pass 把它从 WHERE 提取出来后剩余条件继续下推到 `ParquetExec`。

**缓存**：解压后的 `Index` cache 进 `caching::ParquetMetadataCache` 同款 LRU（key = object_key）；并发查询同 file 不重复下载。

**替代方案**：把倒排数据写到 parquet 的 Bloom filter 列 —— 否决：Bloom 误报率高，对 free-text 不友好。

### 6. 智能缓存层（新 `caching` capability）

三个独立 LRU 各自 `moka::sync::Cache`：

| Cache | Key | Value | TTL | Capacity |
|---|---|---|---|---|
| `parquet_file_meta` | `(org, stream, stream_type, time_bucket_hour)` | `Arc<Vec<ParquetFileMeta>>` | 60s | 100k 桶 |
| `parquet_meta` | `object_key` | `Arc<ParquetMetaData>` | 600s | 10k |
| `query_result` | `blake3(stmt + org + time_range + role)` | `Arc<QueryResult>` | 60s（仅 SQL，PromQL 不缓存）| 1k |

**取消缓存条件**（write-through invalidation）：
- `ParquetFileMetaRepository::insert/replace/mark_deleted` 触发 `parquet_file_meta_cache.invalidate_by_prefix((org, stream, stream_type))`。
- `parquet_meta` 永不主动失效（object_key 是 KSUID 不会复用）。
- `query_result` 仅对包含 `_timestamp >= now - 5min` 的查询禁用（频繁变动窗口）。

**指标**：每级缓存导出 `cache_{level}_{hits|misses|evictions}_total` Counter；hit_ratio 进 `/metrics`。

**响应增强**：`QueryResult` 加 `cache_hit: bool`，让上层判断缓存是否生效。

**替代方案**：单层 LRU 包所有 key —— 否决：不同对象 TTL 与 size 差异大，分层利于 tuning。

### 7. Alert manager：周期评估 + 派发

`AlertManagerWorker::run`：

```
loop interval(eval_interval_secs):
  for rule in rule_repo.list_enabled():
    res = query_engine.execute(rule.query, [now-period, now])
    matches = compare(res.scalar(), rule.trigger)
    state = rule_eval_state_repo.upsert(rule.id, matches)  // 累计 consecutive
    if state.consecutive >= rule.for_periods && open_incident(rule.fingerprint).is_none():
      incident_repo.insert(new_incident(rule, fingerprint))
    elif !matches && incident.is_some():
      incident_repo.update_status(incident.id, Resolved, "system")

loop interval(dispatch_interval_secs):
  for inc in incident_repo.list_open():
    policy = escalation_repo.get(inc.escalation_policy_id)
    if inc.delivered.is_empty(): dispatcher.send_step(inc, policy.steps[inc.current_step])
    elif now - inc.current_step_started_at >= policy.steps[inc.current_step].ack_timeout_secs && !inc.acknowledged_at:
      inc.advance_step_or_loop(policy)
      dispatcher.send_step(inc, policy.steps[inc.current_step])
```

`alert_rule_eval_state` 表新增（rule_id, consecutive_matches, last_eval_at），eval_interval 改动时不重置（向前兼容）。

### 8. Cluster registry + Router

`cluster_nodes` 表：(node_id PK, role, advertise_addr, started_at, last_heartbeat_at)。

每个 role 启动时挂一个 `HeartbeatTask { interval: 5s }` 调 `NodeService.Heartbeat` —— 在 standalone 模式直接写本地 repository，无 gRPC 跳转。

`cluster_registry.list_role(role) → Vec<Node>` 过滤 `last_heartbeat_at >= now - 15s`。

**Router**：基于 `axum` 中间件路径匹配 → `cluster_registry.pick_ingester((org, stream))`（一致性哈希）/ `.pick_querier()`（轮询）→ 反向代理（`reqwest`）。限流用 `governor`，key 是 `(org_id, route_class)`。

**为什么不接 service mesh / dns SRV**：本期内嵌注册表足够；后续可换 etcd/consul，对 trait 抽象（`ClusterRegistry trait`）。

### 9. 多租户系统化加固

**Planner pass**：在 DataFusion 注册 `RewriteTableNamesPass` —— 把 `FROM <stream>` rewrite 成 `(SELECT * FROM <stream> WHERE org_id = '<runtime_org>')`。这一步用 DataFusion 的 LogicalPlan rewriter 实现。

**Repository 层签名**：所有 `find / get / list` 第一参数强制 `org_id: Id`；现有 14 个 repo 的 trait 一一审查 —— ingestion / storage / dashboard 已经有，alerting / identity 系列部分缺。本 change 补齐。

**额外检查**：`Permission::require` 在 handler 入口已经做 role check，新增一层 ownership check：handler 把 path :id 加载实体后必须验证 `entity.org_id == ctx.org_id`，否则 `404 Not Found`（不暴露存在性）。

### 10. 无状态保证（约束化）

**单点本地状态**：
- ingester：`wal.dir` 是唯一本地持久化（其它都在 meta_store + object_store）。WAL 段命名 `wal-{seq:06}.seg` 在 ARCHITECTURE 已定义，segment 内每条记录含 `(term, index, payload, crc32c)`，崩溃后 mmap 扫描自动截断尾损坏。
- 其它 role（querier / compactor / alert_manager / router）：完全无本地状态。重启 = 重新连 DB + 重新 join cluster registry。

**断言**：在 `IngesterWorker::recover()` 完成前不开 ingest 端口；查询路径在 `cluster_registry.is_role_ready(self)` 之前对外返 503。

### 11. HTTP API 全量上线（含 Stream / Dashboard / Alert / Schedule / SavedView / Function / Pipeline）

**选择**：所有 handler 落在 `crates/api/src/http/<resource>.rs`，请求与响应负载放同目录的 `<resource>_request.rs` / `<resource>_response.rs`（**文件名不出现 `dto`**；类型命名用 `CreateStreamRequest` / `StreamResponse` 等业务语义名）。

**统一 handler 形态**：

```rust
pub async fn create_stream(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(req): Json<CreateStreamRequest>,
) -> Result<(StatusCode, Json<StreamResponse>), Error> {
    ctx.require(Permission::StreamAdmin)?;
    let req = req.validate()?;                   // 字段级校验
    let stream = state.ingestion.create_stream(ctx.org_id, req.into_domain()).await?;
    Ok((StatusCode::CREATED, Json(StreamResponse::from(stream))))
}
```

每个 `*Request` 实现 `Validate` trait（自家 trait，非 third-party `validator`，保持依赖纯净），`into_domain(&self, org_id) -> DomainModel`；每个 `*Response` 实现 `From<DomainModel>`。validator 失败统一返 `Error::Invalid("field <X>: <reason>")`。

**新资源 schema 草案**：

| 资源 | 路径 | 关键字段 |
|---|---|---|
| Stream | `/api/v1/streams` | `name, stream_type, schema (Vec<FieldDef>), retention { days, hot_days? }, indexed_fields[]` |
| Alert Rule | `/api/v1/alerts/rules` | `name, query { language, statement, time_range_secs }, trigger { op, threshold }, for_periods, silence_secs, escalation_policy_id, enabled, labels` |
| Escalation Policy | `/api/v1/alerts/policies` | `name, steps[{ targets: [{kind, id}], ack_timeout_secs, channel_ids[] }], repeat, max_loops` |
| Channel | `/api/v1/alerts/channels` | `name, kind, config { slack: { webhook_url }, email: { to[], subject_tmpl }, webhook: { url, headers, method } }` |
| Schedule | `/api/v1/schedules` | `name, timezone, rotations[{ name, cadence: { period_days, handoff_at }, members[], active_windows[{ weekday_mask, hour_start, hour_end }] }], overrides[{ user_id, start, end }]` |
| Dashboard | `/api/v1/dashboards` | `folder_id, model (Grafana JSON 透传)`，create 接受 `{ source: "native" \| "grafana", payload: ... }` |
| Folder | `/api/v1/folders` | `name, parent_id?` |
| Saved View | `/api/v1/saved_views` | `name, language, statement, time_range_secs, stream?, tags[], pinned` |
| Function | `/api/v1/functions` | `name, language: "vrl"\|"js", source, input_schema_hint?, output_schema_hint?` |
| Pipeline | `/api/v1/pipelines` | `name, stream_targets[{org_scoped_stream}], steps[{ function_id, params }], enabled` |

**分页约定**：所有 list 接受 `?page=1&page_size=50&filter=<name_substring>`，响应 `{ items, total, page, page_size }`；默认 `page_size = 50`，上限 `200`。

**权限矩阵**（精简）：所有写路径需 `Editor`+；删除需 `Admin`+；列表/读 `Viewer` 即可。Stream / Pipeline / Function 的写还要加 `Permission::DataPlaneAdmin`（新增枚举值）。

**替代方案**：把 request/response 类型集中到 `crates/api/src/payloads/`—— 否决：用户明确要求避免 dto 风格集中目录，按资源就近放更符合 axum 习惯。

### 12. Functions / Pipelines / Saved Views 执行模型

**Functions**：domain 上是 `Function { id, org_id, name, language, source, params_schema }`；执行经 `FunctionRuntime trait`，默认两种实现：

- `VrlRuntime`：基于 `vrl` crate（Vector 同源），把 `Event` 转 `vrl::value::Value` 跑 program 后回写；预编译 + 缓存编译产物（key = `(function_id, version)`）。
- `JsRuntime`（feature `js`）：基于 `boa_engine` 起 sandbox，限 cpu_time / memory（通过自实现 host hook + `Realm` cap），默认不开启。

**Pipelines**：`Pipeline { id, org_id, name, steps: Vec<PipelineStep { function_id, params }>, stream_targets }`；ingester 在 `IngestService::ingest` 的 schema 校验之前插一步 `pipeline_engine.apply(batch, stream)`：

```
fn apply(&self, batch: &mut IngestBatch, stream: &StreamDefinition) -> Result<()> {
    let Some(pipeline) = self.lookup_for_stream(stream) else { return Ok(()) };
    for step in pipeline.steps {
        let function = self.functions.get(step.function_id)?;
        let runtime = self.runtimes.get(function.language)?;
        runtime.transform_in_place(batch, function, &step.params)?;
    }
    Ok(())
}
```

错误处理：单条 event transform 失败 → 该 event 加进 `rejected` 列表（与 schema 校验失败同处理）；整 batch 异常（编译错误等）→ 整批 400。

**Saved Views**：纯持久化 + 一个执行入口 `POST /api/v1/saved_views/:id/run?step=&range=`，server 端把保存的 `(statement, time_range_secs, stream)` 重新组成 `QueryRequest` 走标准 query 路径。pin 状态用于前端置顶（本期仅存字段，不影响后端逻辑）。

**Sandbox 资源限制**：`pipeline.cpu_time_ms_limit / memory_mb_limit / wall_time_ms_limit` 通过 `tokio::time::timeout` + VRL 自带 step counter 限制 CPU；javascript runtime 跨线程隔离 + thread-local CPU budget。超限 → 该 event 进 `rejected`，metric `pipeline_function_limit_exceeded_total{function_id,reason}` 增。

**替代方案**：把 pipeline 落到 `app` 层而不是 `infra`—— 否决：function runtime（VRL / JS）是基础设施依赖，更适合 infra；app 层只编排。

### 13. 对象存储生产化接入

**选择**：在 `crates/infra/src/storage/object.rs` 抽出 `ProductionObjectStore` 装饰器包住底层 `object_store::ObjectStore`，对外仍是 `ObjectStoreExt` trait（已存在），但内部新增：

```rust
struct ProductionObjectStore {
    inner: Arc<dyn object_store::ObjectStore>,
    backend: BackendKind,
    settings: ObjectStoreSettings,
    semaphore: Arc<Semaphore>,         // 限并发
    retry_policy: RetryPolicy,
    metrics: ObjectStoreMetrics,
}
```

**Multipart upload**：

```
pub async fn put_large(&self, key: &str, bytes: Bytes) -> Result<()> {
    if bytes.len() < self.settings.multipart_threshold_mb * MIB {
        return self.inner.put(key, bytes).await.map_err(into_obs);
    }
    let parts = bytes.chunks(self.settings.multipart_part_size_mb * MIB);
    let upload = self.inner.put_multipart(key).await?;
    let mut futs = FuturesUnordered::new();
    for (i, part) in parts.enumerate() {
        let permit = self.semaphore.clone().acquire_owned().await?;
        futs.push(async move {
            let _permit = permit;
            upload.put_part(i, part).await
        });
    }
    let parts: Vec<_> = futs.try_collect().await?;
    upload.complete(parts).await?;
    Ok(())
}
```

`object_store` crate（≥ 0.10）原生提供 `put_multipart`，按 backend 自动选 AWS multipart / Azure block blob / GCS resumable / local 临时文件。本期不重发明，包一层并发 + retry。

**Range 下载**：`get_large(key, length_hint)`：当 `length_hint > range_threshold_mb`（默认 16 MiB），按 `range_chunk_mb`（默认 8 MiB）切片并发 `get_range` 拼回 `Bytes`；否则单次 `get`。

**重试策略**：

```rust
pub struct RetryPolicy {
    pub max_attempts: u8,            // default 4
    pub base_backoff_ms: u64,        // default 100
    pub max_backoff_ms: u64,         // default 5_000
    pub jitter_ratio: f32,           // default 0.2 → 实际 wait ∈ [base*(1-r), base*(1+r)]
    pub retryable: fn(&Error) -> bool,
}
```

`retryable` 默认匹配：`object_store::Error::Generic { source }` 含 `5xx`、`SlowDown`、`Throttling`、`Timeout`、`NotConnected`，其余视为 permanent。

**操作超时**：每次原子 op 包一层 `tokio::time::timeout(op_timeout_secs)`，默认 30s。

**指标**：每次操作记 `object_store_operations_total{backend, op}`、`object_store_bytes_total{backend, op}`、`object_store_errors_total{backend, op, reason}`、`object_store_op_duration_seconds_bucket{backend, op}` 直方图。

**Health Check**：

- **启动 ping**：`wire::build_state` 在 object store 构造后立即 `put → get → delete` 一个 `_health/<uuid>` 小对象（128 字节），失败直接 `Err`，进程不启动。
- **持续探活**：`/api/v1/healthz` 探针每 30s 做一次同样的轻量 round-trip，记录 `object_store_health_check_duration_seconds`；连续 3 次失败 → `/api/v1/healthz` 返 `503` + body `{ "status": "degraded", "reason": "object store unreachable" }`，仍允许 `/metrics`。

**凭证**：

- 静态 AK/SK：`[object_store] access_key / secret_key` 直接配（仅 S3-like backend 需要）。
- 环境变量：标准 `MS_OBJECT_STORE_ACCESS_KEY / MS_OBJECT_STORE_SECRET_KEY` 覆盖（全部使用单下划线，与既有 env 一致）。
- 凭据文件：`credentials_file = "/run/secrets/molesignal-objstore"`，文件首行 `access_key=...\nsecret_key=...` 启动时读入。

不引入云厂商凭据链（IAM Role / Managed Identity / Workload Identity），保持简单：

**为什么不接云原生凭证**：用户明确要求只支持静态 AK/SK；同时降低运维复杂度，云原生凭证留独立 change。

**替代方案**：直接用 `object_store` 自带的 retry middleware —— 否决：自带 retry 不暴露 jitter / 自定义 retryable 判断，包一层装饰器更可控。

### 14. 多协议 ingest receivers

每个协议在 `crates/api/src/http/ingest_<protocol>.rs` + `crates/api/src/grpc/<protocol>_server.rs` 落 receiver，统一 normalize 成 `IngestBatch` 后调 `IngestService::ingest`。

| 协议 | 入口 | normalize | 关键约束 |
|---|---|---|---|
| OTLP gRPC | `grpc::otlp_{logs,metrics,traces}_server.rs` | resource/scope → labels；`severity_number` → level；`time_unix_nano` → micros | proto crate `opentelemetry-proto` |
| OTLP HTTP | `POST /api/v1/{logs,metrics,traces}` | 解析 protobuf 或 application/json 同 schema | 复用 gRPC 的转换函数 |
| Prom remote_write | `POST /api/v1/prometheus/api/v1/write` | snappy 解 → `prometheus.WriteRequest` → 每个 TimeSeries 拆分 → metrics 流（按 `__name__`） | `snap` + `prost` |
| ES `_bulk` | `POST /api/v1/_bulk` | NDJSON 双行（action + source）解析；index name → stream name；返回 ES-compat response | 兼容 Vector / Fluent Bit ES output |
| Loki push | `POST /api/v1/loki/api/v1/push` | snappy-protobuf 或 JSON；按 `service_name`/`job` 解 stream | `logproto` proto |
| Syslog | UDP/TCP listener tasks | `syslog-loose` 双 RFC 解析；目标 stream `[syslog].default_stream` | 在独立 tokio task 启 |
| Kinesis Firehose | `POST /api/v1/_kinesis_firehose` | Base64 解 + 行分裂；返回 `{requestId, timestamp}` | 兼容 AWS Firehose HTTP delivery |

所有协议受 `quotas` 模块限制（per org ingest QPS / bytes）；audit 模块对 ingest 不写 per-call 行（量大走 metric）。

### 15. Real-time alerts

`RealTimeAlertCompiler` 在 alert_manager 启动时把每条 `RealTime` 规则的 `matcher`/`where_sql` 编译成 `Arc<dyn EventPredicate>`（VRL 或 DataFusion `Expr`），通过 `tokio::sync::watch` 推到所有 ingester 进程内的 `RealtimeMatcherCache`（按 stream 索引）。`IngestService::ingest` 在 WAL append 之后用 `RealtimeMatcherCache::matches(stream, &record)` 对每条记录跑判定，命中 → `IncidentEvent` 经 `tokio::sync::broadcast` 给本进程的 alert_manager subscriber（standalone 模式）或经 gRPC `cluster.IncidentService::Publish` 上报。Dispatcher 复用现有 `EscalationDispatcher` 链路。延迟目标：从 record 到首通知 < 1s（不含人工 ack）。

### 16. API tokens + Audit + Quotas（控制面三件套）

- **API tokens**：`api/src/http/middleware/auth.rs` 已有 JWT 路径；这里加一段 prefix 判断 `ms_` → 走 token 查表（按 prefix 索引）+ argon2 验 secret + AuthContext 注入；`last_used_at` 用 `tokio::spawn` 异步 update 不阻塞请求。
- **Audit**：`api/src/http/middleware/audit.rs` Tower layer，在 response 出来后把 `(actor, action, target, status_code, payload_json)` 异步写 `audit_events`（`tokio::spawn` + 失败只记 metric）。`audit_events` 表加 `(org_id, ts DESC)` 索引。
- **Quotas**：`crates/infra/src/quotas/limiter.rs` 持有全局 `DashMap<org_id, Arc<RateLimiter>>` 用 `governor`；ingest / query 入口在权限校验后立刻 `acquire_async`；后台任务每 30s 从 DB 重读 quotas 表，差异 patch 进 limiter；storage bytes 由 compactor 每 5 min 汇总并写 `(org_id → bytes)` 进程内地图。

### 17. Service graph（traces 派生）

`crates/infra/src/traces/service_graph.rs::ServiceGraphAggregator`：进程内 `DashMap<(org_id, client, server, time_bucket_min), Stats>`；ingester 在写 traces 流之后同步 emit `(spans → edges)`（parent_id 链接缺失的 span 直接忽略）。每分钟边界由 `compactor` 同进程 task（或独立 task）`flush_to_db()` upsert `service_graph_edges` 表。读路径直接 SQL 查表 + 内存当前未 flush 桶合并返回。

### 18. Anomaly detection

`AnomalyDetector` trait + 三实现：
- `MadDetector`：fetch `[now - lookback_days*1d, now)` 与 `now` 同分钟桶的历史值（一次 DataFusion 查询 `WHERE _timestamp = bucket_of_each_day`），算 median + MAD，比对当前；`unimplemented` 的两个 detector 仅返错。
- 评估在 alert_manager 周期 tick 内对 `kind = "anomaly"` 的 rule 调；其它 rule 走原 scheduled 路径。
- 历史拉取走 `caching::query_result` 缓存（同 SQL 命中减扫描）。

### 19. LLM telemetry

OTLP traces ingester 加 `LlmFanoutHook`：识别 span attribute key `gen_ai.system / gen_ai.request.model / gen_ai.usage.*`，把对应字段抽出 → 写一份 `llm_traces`（其余字段保留）。默认 pipeline 在 `llm_traces` 之上预绑 `redact_pii` VRL function（识别 email / phone / credit-card 用正则替换）；org 可经 `[llm].redact_function_id` 覆盖。派生查询 endpoints 直接走 saved-view-like 模板（在 `crates/app/src/llm/queries.rs` 提供）。

### 20. RUM 接收

新 HTTP 模块 `api/src/http/rum.rs`，按 Datadog RUM 兼容 schema 接收（最广泛的开源参考契约）；session / action / error 三个流走标准 `IngestService::ingest`；replay 流走 `RumReplayWriter`：按 session 累计 buffer，达到阈值或 session 结束（`session_ended_at` 字段）落 `rum/<org>/<session>/<seq>.replay.ndjson.zst` 到 object_store + 在 `rum_replay_events` 表插指针行。Session-trace correlation 在 query 时 join。

### 21. SSO（OIDC + SAML）

- **OIDC**：`openidconnect` crate；handler `auth/sso/login` 构 auth request 重定向；`auth/sso/callback` 跑 `IdTokenVerifier::verify_id_token` + nonce 校验；user provision via `IdentityService::provision_or_get(email, role)`。
- **SAML**：`samael` crate；handler 解 `SAMLResponse` POST → 验签（IdP cert from `idp_metadata_url`）→ 抽 `NameID` 与 group attribute → provision/get user。
- **Role mapping**：`[auth.sso].role_mapping = [{ idp_group: "x-editors", role: "editor" }]`；每次登录跑映射（不降级）。
- **License gate**：community license 下 SSO 端点返 `403 feature 'sso' requires enterprise license`。

### 22. Federated search

`crates/infra/src/query/federated.rs::FederatedDistributedEngine` 包 `DistributedDataFusionEngine`：先看 request 的 `clusters` 列表，本地集群走现有路径；每个 remote cluster 拿其 `advertise_addr + bearer token` 直接调 Arrow Flight `do_get`（同 in-cluster fan-out 协议）→ 汇总到 coordinator 后跑 final SQL。不可达 cluster → 在 `meta.degraded_clusters` 列出，整体不 fail。

### 23. Cipher keys + 字段级加密

`crates/infra/src/cipher/cipher_keys.rs`：master key 来自 env，AES-256-GCM；新增的 key material 用 master key 包裹再落库（`key_material_enc`）。VRL 内置 `encrypt(value, key_id)` / `decrypt(value, key_id)` 实现读 `cipher_keys` 拿明文 key → 加/解密 → 输出 `kid:<id>:v<n>:<base64>` 字符串前缀。版本号嵌入前缀，解密时直接选对应版本。

### 24. License

`crates/infra/src/license.rs`：启动读 file → 验 Ed25519 → 解 payload。Feature flag 通过 `License::has_feature(name)` 给各模块查；SSO / federated / LLM fanout 启动时 fail-closed（无 license 默认 community 不启用）。每日 ingest bytes 用 atomic counter，到点（UTC midnight）落 `license_usage_daily` 表；超 cap 返 429。

### 25. Cloud connectors

`crates/infra/src/connectors/`：`Connector trait` + 4 个 impl（cloudwatch_logs / kinesis_firehose / cloudflare_logpush / heroku_drain）；新 role `connector`（也可与 alert_manager 合并）每 `poll_interval_secs` 扫 `connectors WHERE enabled = true AND kind in pull_set`，并发跑 `connector.pull(ctx)`；push connector 共享 `api/src/http/connectors_push.rs` 路由（每 kind 一条），统一鉴权 `X-Connector-Token`。

## Risks / Trade-offs

- **[Arrow Flight 反序列化与 DataFusion 类型对齐]** → 用 `arrow_flight::flight_data_to_arrow_batch` 标准路径，避免手卷；Ticket schema 与本地一致；端到端测覆盖两节点 + 时间窗 partial scan。
- **[Tantivy 索引文件大小过大]** → 仅对 `indexed=true` 字段建；index 不重写 parquet，使用 tar+zstd 控制体积；目标 < parquet 自身 10%；测试加 size assertion。
- **[PromQL evaluator 与 Prometheus 语义偏差]** → 文档 `docs/promql_subset.md` 列支持函数与已知差异；每个支持函数单测对 Prometheus reference 行为。
- **[query_result_cache 命中陈旧]** → 仅缓存包含明确 `_timestamp <= now - 5min` 的查询；前端实时面板默认 60s 自刷新本就接受 60s 偏差；handler 增 `Cache-Control: no-store` 头部直通选项。
- **[多租户 planner rewrite 漏 SQL 子查询路径]** → 用 DataFusion `LogicalPlan::transform_down` 全树重写，覆盖 join / cte / subquery；测试 `it_multitenant` 包含 join + cte + window 三种形态。
- **[一致性哈希在 ingester 扩缩容时 reshard 数据归属]** → 本期接受：reshard 后受影响 `(org, stream)` 在新 ingester 上重新走 schema 自动建链路；旧 ingester 上未 flush 的 WAL 会随其下线 flush 完成（drain 阶段，本期通过 SIGTERM hook + 5min grace 实现，简化版）。
- **[OTLP exporter 与 opentelemetry crate 版本耦合]** → 单独锁版本组合（`opentelemetry 0.24` / `opentelemetry_sdk 0.24` / `opentelemetry-otlp 0.17` / `tracing-opentelemetry 0.25`），workspace 统一管理；CI 加 `cargo deny ban` 防偏移。
- **[alert_manager evaluator 长查询拖累 tick]** → 每条 rule 评估走 `tokio::time::timeout(eval_timeout_secs)`，默认 10s；超时记 metric `alert_rule_eval_timeout_total` 不阻塞下一轮。
- **[router 跨域代理 streaming body 占内存]** → 反代直接 `tokio::io::copy_bidirectional` reqwest 的 `bytes_stream`，不缓冲整 body。
- **[VRL / JS function 沙箱逃逸]** → VRL 本身是 DSL 无 IO，安全边界天然窄；JS 默认关闭，仅在 `feature = "js"` 启用时纳入；boa runtime 在独立 OS thread + memory cap + step counter；任何 panic 进 `tokio::task::spawn_blocking` 隔离。
- **[Multipart upload 中途失败留垃圾]** → `put_multipart` 返回的 `UploadId` 在异常分支 abort（`MultipartUpload::abort`）；object_store crate 不暴露 abort 时退化为：记 `object_store_multipart_orphan_total` 并在 retention sweep 时一并扫 `_uploads_pending/` 前缀（本期仅打点，未来 change 扫除）。
- **[健康检查的 round-trip 自身耗带宽]** → 探针对象大小固定 128 字节、key 前缀 `_health/`、`compactor` retention sweep 显式排除，影响可忽略。
- **[HTTP API 入侵式签名 breaking]** → 既有 handler 返 `400 "TODO ..."` 的契约本就未生效；新签名是首次落地，不算 breaking。客户端文档与 README 在 16.x 一并更新。
- **[Real-time matcher 增加 ingest 路径耗时]** → 编译产物 `Arc<dyn EventPredicate>` 跑一条 record 通常 < 10 μs；按 stream 索引避免 N×M 比对；预算 < 1 ms/batch。
- **[Anomaly detector 历史拉取 cost]** → 仅 MAD detector；同分钟桶查询命中 `query_result` 缓存；超时 10s 同 scheduled 评估。
- **[LLM PII redaction 误伤普通字段]** → 仅对 `gen_ai.prompt / completion` 字段跑；原 traces 流不动；redact 函数默认保守只匹配 email/phone/cc。
- **[SAML 实现复杂]** → 仅支持 POST-binding + IdP-initiated metadata fetch；不实装 SLO、不接 NameIDPolicy 全集。
- **[Federated search 跨集群 auth 泄露]** → token 仅经 `cipher_keys` 或 env 引用，永不在 API 响应回显；mTLS 默认开启 `tls_verify=true`。
- **[Cipher key master 丢失数据不可恢复]** → 文档明确说明运维必须备份 master key；`MS_MASTER_KEY` 缺失启动失败、不静默降级。
- **[License signature 私钥被盗造假 license]** → root pubkey 编进 binary；私钥仅项目自留；license-issuer CLI 单独维护。社区版本不依赖 license（fail-open 路径）。
- **[Cloud connectors 静态 AK/SK 风险]** → 仅静态凭证，运维必须经 secret manager 注入；connector config_json 中的敏感字段 list 化在响应里 mask。

## Migration Plan

1. **依赖升级**：workspace `Cargo.toml` 加 `promql-parser`、`opentelemetry-*` 套件、`prometheus`、`moka`、`governor`、`reqwest` (`rustls`)；先 `cargo build --workspace` 验证版本兼容。
2. **数据库迁移**：新建 `crates/infra/migrations/20260601000001_cluster_nodes_and_eval_state.sql` 含两张表 + 索引。
3. **逐 role 上线**：先 ingester（替换 MemoryIngestSink），跑 it_ingester_flush；再 compactor + retention；再 distributed querier；再 alert_manager；最后 router。
4. **配置默认**：新增段的 `Default` 全部给保守值（buffer_max_mb=64，flush_interval_secs=30，compactor.interval_secs=120，target_mb=128，eval_interval_secs=30，dispatch_interval_secs=10）；`conf/config.toml` 同步示例。
5. **回滚**：在 standalone 模式所有 role 共享一个进程；任一 role 出问题可通过 `[node].roles` 临时关掉该 role；ingester 必须保留（写入路径），不可关闭，但 buffer/flush 失败时仍会持续写 WAL，重启可恢复。

## Open Questions

- **`alert_rule_eval_state` 是否需要在 Incident resolve 时清零 `consecutive_matches`**？倾向"是"，避免 resolve 后下一 tick 再次匹配立刻触发；本期实现按"resolve 即清零"。
- **router 是否复用 querier 的 cluster registry 选举来决定自己是不是主**？本期不选主，所有 router 独立运行；客户端可前面挂任意 LB（本期 k8s manifest 用 ClusterIP）。
- **Tantivy 索引按 file 粒度 vs 按 day 粒度**？本期按 file 粒度（与 parquet 1:1，最简单），day 级聚合留独立 change。
- **query_result_cache 的 key 是否需要包含 user_id**？本期 key 不含 user_id，但 cache 命中前仍重跑 RBAC ownership check；不存在数据泄漏，仅命中率略低。
- **Pipeline 是否允许 fan-out（单条进多条出）**？OpenObserve 是允许的（VRL `.` 重新赋值为 array）。本期暂只支持 in-place transform（不增减条数），fan-out 留下个 change。
- **对象存储 multipart 并发是否要按 backend 调优**？本期统一 `max_concurrency`，未来按 backend（S3 vs Azure block size 限制）拆分参数。
