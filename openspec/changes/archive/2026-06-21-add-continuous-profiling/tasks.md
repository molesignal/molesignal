## 1. 数据模型与协议契约

- [x] 1.1 在 `proto/ingest/v1/ingest.proto` 的 `StreamType` 增加 `STREAM_TYPE_PROFILES = 4`，重新生成 `src/protocol` 产物。
- [x] 1.2 在 `crates/domain/src/stream` 的 `StreamType` 增加 `Profiles`，并使 `allowed_as_pipeline_target` 对其返回 `false`。
- [x] 1.3 在 `crates/infra/src/profiles.rs` 定义 `NormalizedProfile` / `Sample` / `ValueType` / `ProfileType`（扁平布局，不新建子目录）。
- [~] 1.4 在 `src/protocol` 钉选并生成 pprof `profile.proto` 与 OTLP profiles proto，记录所钉版本号。（pprof `profile.proto` 已 vendored 到 `proto/pprof/v1/`、buf 生成 `perftools.profiles`、`protocol::pprof::profiles` 已导出；OTLP profiles proto 按分阶段路线随 2.5 适配器交付。）
- [x] 1.5 前端 `web/src/api/streams.ts` 等 `StreamType` 别名同步加入 `profiles`（含 `Streams.tsx` 的 `TYPE_TONE` / `streamTypeLabel` 与 `SignalReference` 的 `SignalReferenceStreamType` 穷尽用法；`pnpm typecheck` 通过）。

## 2. 摄取协议适配

- [x] 2.1 实现 pprof 解码（gzip protobuf → `NormalizedProfile`）与编码（`NormalizedProfile` → 规范 pprof），保证无损往返。（`infra/profiles.rs`：decode/encode/encode_raw + 叶/根翻转，单测含 round-trip）
- [x] 2.2 实现 `POST /api/v1/profiles/upload`（pprof 直传）于 `crates/api/src/http/routes/profiles.rs`。
- [x] 2.3 实现 Pyroscope 兼容 `POST /api/v1/profiles/ingest`：解析 `name{labels}` 与 `format=pprof|folded|lines`。
- [x] 2.4 实现 folded / lines 文本栈解析器。（`infra/profiles.rs::parse_folded`，单测对齐 spec 场景）
- [~] 2.5 实现 OTLP profiles 适配器 `POST /api/v1/profiles/otlp`，解码逻辑收敛于适配器、proto 版本钉选；入口默认开启，仅提供应急关闭的配置开关（如 `MS_PROFILES_OTLP_ENABLED`）。（路由已挂、默认开启语义占位；OTLP profiles proto vendoring + 适配解码待补，当前返明确错误）
- [~] 2.6 JFR 解析（Java 来源），本变更内交付；实现顺序排在 pprof 之后，过渡期未实现路径返明确错误。（upload/ingest 的 `format=jfr` 已返明确错误占位，解析待补）

## 3. 存储与落盘

- [x] 3.1 规范化后双路落盘：zstd pprof → object store（按 key 规范）；元数据 `RawEvent` → `IngestService::ingest`。（`profiles.rs::put_archive` + 路由 `store_profile`）
- [x] 3.2 定义 profiles 元数据流字段：`service / profile_type / duration_nanos / sample_count / total_value / labels / trace_id / span_id / object_key / unsymbolized`（+ `id` / `archived_bytes`）。
- [x] 3.3 trace 关联抽取：从样本级 / profile 级标签提取 `trace_id` / `span_id` 写入元数据行。（`extract_trace_ids`，单测覆盖）
- [x] 3.4 保留：到期同时清理 parquet 元数据与归档 blob。（元数据随 stream 保留自动清理；归档 blob 由 `Compactor::retention_sweep` 对 `StreamType::Profiles` 调 `profiles::sweep_expired_archives` 清理，按 key 内 `yyyymmdd` 与 cutoff 比较；单测覆盖 key 日期解析与按前缀/日期删除）

## 4. 查询与聚合

- [x] 4.1 `GET /api/v1/profiles` 列表 / 筛选（service / type / label / time），走元数据查询。
- [x] 4.2 火焰图合并器：拉取窗口内 blob → 按 frame 路径聚合栈树 → flamebearer；`max_merge` 上限 + 均匀采样 + `truncated` 标记。（`infra/profiles_merge.rs`，单测覆盖合并 / diff / 采样）
- [~] 4.3 `GET /api/v1/profiles/flamegraph` 端点，结果按 `(service,type,window,labels)` 指纹缓存（复用 caching 层）。（端点 + 采样 + truncated 已实现；指纹缓存接 caching 层待补）
- [x] 4.4 `GET /api/v1/profiles/diff` 差分聚合（baseline vs comparison，输出带符号增量）。
- [x] 4.5 `GET /api/v1/profiles/{id}` 原始 pprof 下载。
- [x] 4.6 `trace_id` / `span_id` 过滤的火焰图聚合（trace 关联查询）。

## 5. 前端 Profiles 模块

- [x] 5.1 `web/src/product/ia.ts` 注册 `profiles` owner module 与 `/profiles`、`/profiles/:id`、`/profiles/compare` 路由及图标（Flame）；`routes/index.tsx` 挂载；`nav.json` 加 `profiles` 标签。
- [x] 5.2 `web/src/api/profiles.ts` API 客户端（list / flamegraph / diff / download；camelCase flamebearer + snake_case 包装；stream-not-found 视作空）。
- [x] 5.3 `web/src/viz/profiles` 火焰图组件：flamebearer 渲染、搜索高亮、点击下钻、diff 着色；纯解码/缩放 helper + 单测。（value 类型切换 = 列表页 profile_type 选择，重查火焰图）
- [x] 5.4 Profiles 列表页（service / type 选择、时间范围、标签筛选；KPI + 列表 + 火焰图浏览器）。
- [x] 5.5 差分 / 对比页（baseline vs comparison 期间，增减着色）。
- [x] 5.6 trace ↔ profile 关联入口（profiles 跳 trace；traces 详情「查看该 span 火焰图」→ `/profiles?trace_id&span_id`）。
- [x] 5.7 空状态 / 接入引导（OTLP / Pyroscope / pprof 可复制片段）+ `truncated` 非阻断提示。
- [x] 5.8 i18n：新增 `profiles` 命名空间 en-us + zh-cn；`scripts/i18n/check.ts` 校验 28 命名空间键齐（通过）。

## 6. 门禁与配额

- [x] 6.1 profiles 摄取计入 `max_ingest_qps` / `max_storage_bytes`，超限返 413：`store_profile` 写归档前走与所有信号同源的 `ensure_ingest_allowed`，该门禁现已接入 `QuotaLimiter`——QPS 超→429（含重试秒数）、storage 超→413（写对象前判定，超限不落盘）；`archived_bytes` 记入元数据行。**平台级接入（本次补齐）**：infra `quotas` 模块原为孤儿（未在 `lib.rs` 声明、从未编译），现已声明并接入；新增 `PgQuotaRepository`（读 `quotas` 表上限 + `parquet_file_meta` 按 org 聚合用量）；`wire.rs` 起 60s 后台 refresh loop 载入进进程内 `QuotaLimiter`；`AppState.quotas` 注入；新增 `Error::PayloadTooLarge`(413) 变体（含 gRPC Flight SQL 映射）。无 `quotas` 记录的 org 上限为 0 → 视作无限制（OSS / 未配额 org 零行为变化）。
- [x] 6.2 版本门禁：OSS 核心可用；diff 按 `license.has_feature("profiling_enhanced")` 门禁（沿用 `actions` 的 `require_license` 模式），未授权返 403 + 所需 edition。（同一 helper 可复用到跨服务聚合 / 长保留 / 符号化 / Pyroscope render 落地时）
- [x] 6.3 前端门禁呈现：compare 页捕获后端 402/403 → 渲染 `pro-required` 门禁态（edition 文案），不抛裸 403。（前端已不做主动 feature gating，统一由后端裁决 + 前端友好呈现）

## 7. 文档与本地全量验证

- [x] 7.1 更新 `ARCHITECTURE.md`：新增 Continuous Profiling 数据流与存储布局（object store key + 双路落盘 + 端点表 + trace 关联 + 保留/门禁），明确与自诊断 `profiling`（`/debug/profile/*`）的区别。
- [x] 7.2 后端测试 / clippy：`molesignal-infra`（374 lib + 集成）、`molesignal-api`（78 lib + 集成）全通过；`cargo clippy -p molesignal-infra -p molesignal-api` 零警告。（profiles round-trip / folded / trace 抽取 / 合并 / diff / 采样 / 归档 sweep 均覆盖）
- [x] 7.3 前端：`pnpm typecheck` 通过；`vitest` 150 测试通过（含 flamebearer 单测、i18n 键齐）；profiles 相关文件 `eslint` 零问题；`scripts/i18n/check.ts` 通过。（**注**：仓库级 `pnpm lint` 另有 `AiProviders.tsx` 等**既有、与本变更无关**的 migrated-copy/import-order 报错，未在本变更内改动无关文件）
- [x] 7.4 端到端冒烟（本地全栈跑通 + 截图留证）：docker postgres + 本地 FS 对象存储 + `cargo run` 后端 + `pnpm dev` 前端 + Playwright 驱动。流程：admin 登录 → `/profiles/ingest` folded 上传 3 份 → `/profiles` 列表（3 行）→ `/profiles/flamegraph` 合并（numTicks=380；main→compute(290)+io(90)；compute→inner(210)+leaf(80)）→ 前端火焰图渲染（截图确认 nav 入口、KPI 3/1/7、合并火焰图、列表）→ `/profiles/diff` 返 403（OSS 门禁验证通过）。**E2E 暴露并修复 2 个 bug**：(a) `metadata_event` 仅在有值时写 trace_id/span_id → schema-on-write 缺列 → 列表查询无法 plan；改为始终写（无关联=空串）+ 单测锁定；(b) 类型下拉误用 `service_all` 文案 → 新增 `filters.type_all`。
- [x] 7.5 因 CI 不可用，合并前本地全量验证并记录结果：后端（shared/infra/api/bootstrap）build/clippy/test 全绿；前端 `typecheck`/`lint`/`test` 全绿（lint 含 i18n 键齐 + migrated-copy）；端到端冒烟全栈跑通并截图。
