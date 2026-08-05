# Design — Continuous Profiling（持续性能分析）

## Context

平台现有四类遥测信号共享同一条管线：HTTP / gRPC 摄取入口把各类 payload 解析为统一的 `RawEvent { timestamp, fields }`，组成 `IngestBatch { stream_type, events, ... }`，经 `IngestService::ingest`（drain 检查 → schema-on-write 建流 → pipeline → masking → sink）落到 WAL → parquet（object store），traces 另有 `with_service_graph` 旁路派生服务图。`StreamType` 枚举（`Logs / Metrics / Traces / Extend`）在 proto、domain、前端三处镜像。

Continuous Profiling 的数据形态与前三者不同：一份 profile 是**一组带权栈样本**——每个样本是 `(stack: [frame...], values: [cpu_nanos | alloc_bytes | ...], labels)`，火焰图即按栈前缀对样本求和。把它纳入平台的核心问题是：**如何在尽量复用现有摄取 / 存储 / 查询 / 配额基础设施的同时，保留 profile 的保真度并支持高效的火焰图聚合。**

## Goals

- profiles 成为一等遥测流（`StreamType::Profiles`），走统一摄取管线。
- 三种摄取来源——OTLP Profiles、Pyroscope 兼容、pprof / JFR 直传——汇聚到单一规范化表示。
- 火焰图浏览、差分对比、与 trace 互相下钻的端到端体验。
- 复用既有存储、保留、配额、多租户、鉴权。

## Non-Goals

- 不替代现有 `profiling`（`/debug/profile/*`，本服务自诊断）能力。
- MVP 不做服务端符号化（symbolication）服务；假定上报方已符号化（主流 SDK 默认如此）。
- 不内置语言侧采集 agent / SDK；仅做服务端接收，复用生态既有采集器（OTel eBPF profiler、Pyroscope SDK、`runtime/pprof` 等）。
- 不在本变更引入 Pyroscope 查询协议的完整出口（`/pyroscope/render` 兼容出口列为后续 / 增强项）。

## Architecture Overview

```
  采集侧(不在本变更)                服务端(本变更)
  ┌──────────────┐
  │ OTel SDK/    │  OTLP profiles  ┌───────────────────────┐
  │ eBPF profiler│ ───────────────▶│ otlp_profiles handler  │
  └──────────────┘                 ├───────────────────────┤   ┌──────────────────┐
  ┌──────────────┐  /ingest pprof  │ pyroscope handler      │──▶│ ProfileNormalizer │
  │ Pyroscope SDK│ ───────────────▶│ (pprof/folded/lines/jfr)│   │  →NormalizedProfile│
  └──────────────┘                 ├───────────────────────┤   └─────────┬────────┘
  ┌──────────────┐  upload .pprof  │ profiles upload handler│             │
  │ pprof/JFR file│ ──────────────▶│                        │             ▼
  └──────────────┘                 └───────────────────────┘   ┌──────────────────────┐
                                                                 │ 落盘(两路)            │
                                       ┌─────────────────────────┤  1) 规范 pprof+zstd   │
                                       │                         │     → object store    │
                                       ▼                         │  2) 元数据 RawEvent    │
                          ┌────────────────────────┐            │     → IngestService    │
                          │ object store            │            │       → parquet stream │
                          │ profiles/<org>/<svc>/...│            └──────────────────────┘
                          └───────────┬────────────┘                         │
                                      │  blobs                               │ metadata rows
                                      ▼                                       ▼
                          ┌─────────────────────────────────────────────────────────┐
                          │ Query/Aggregation                                          │
                          │  /api/v1/profiles            列表/筛选(查询引擎+parquet)    │
                          │  /api/v1/profiles/flamegraph 窗口内 blob merge → flamebearer│
                          │  /api/v1/profiles/diff       两窗口/两标签集相减            │
                          │  /api/v1/profiles/{id}       原始 pprof 下载               │
                          └───────────────────────────┬─────────────────────────────┘
                                                       ▼
                                          web-profiles（火焰图浏览器 / diff / 关联）
```

## Decision 1 — 存储模型：归档 blob + parquet 元数据（混合）

权衡了三种方案：

- **A. 行级展开**：每个样本拍平成 parquet 一行（`stacktrace`、`value`、`labels`）。纯 SQL 可聚合，复用全链路；但行数随栈基数爆炸、符号字符串高度重复、列存对"栈"这种嵌套结构不友好。
- **B. 纯 blob**：原始 profile 全量存 object store，无结构化元数据。保真、省存储；但无法按 service / label / 时间 / trace 检索"有哪些 profile"。
- **C. 混合（选定）**：原始 profile 规范化为 pprof 后 zstd 归档到 object store（保真、可导出、可被外部工具消费），同时为每份 profile 写一条**元数据行**进 `StreamType::Profiles` 的 parquet 流。

选 **C**，理由：与平台既有"统一事件流 + object store 旁路归档"的哲学一致（RUM replay 即 `rum/<org>/<session>/<seq>.replay.ndjson.zst` 归档 + 元数据行）。元数据行让"列出 / 筛选 profile"复用现有查询引擎、保留、配额；火焰图聚合走专门路径读 blob（火焰图本就不是关系查询，不该硬塞进 SQL）。

**元数据行 schema（`StreamType::Profiles` 流字段）**：

| 字段 | 说明 |
|---|---|
| `timestamp` | profile 时间窗口起点（micros） |
| `service` | 服务名（来自 OTLP resource / Pyroscope `name`） |
| `profile_type` | `cpu` / `alloc_space` / `alloc_objects` / `inuse_space` / `lock` / `wall` / ... |
| `duration_nanos` | 采样时长 |
| `sample_count` | 样本数 |
| `total_value` | 该 type 的样本值合计（用于排序 / 概览） |
| `labels` | 维度标签 map（如 `pod`, `version`, `region`） |
| `trace_id` / `span_id` | 关联的 trace（可空） |
| `object_key` | 归档 blob 的 object store key |
| `unsymbolized` | 是否含未符号化帧（bool） |

**object store key 规范**：`profiles/<org_id>/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`。

## Decision 2 — 摄取：三协议 → 单一规范化层

引入内部规范表示 `NormalizedProfile`（贴近 pprof 语义，便于无损往返）：

```
NormalizedProfile {
  service: String,
  profile_type: ProfileType,
  sample_types: Vec<ValueType>,   // (type, unit)
  samples: Vec<Sample>,           // Sample { stack: Vec<Frame>, values: Vec<i64>, labels }
  period_type, period,
  start_time, duration,
  labels: Map,                    // profile 级标签
  trace_id, span_id: Option,
}
```

三个适配器把外部格式归一到它：

- **pprof / JFR 直传适配器**：pprof 用 `prost` 从 `profile.proto` 生成的类型解码（gzip protobuf）；JFR（Java 生态格式）在本变更内交付，实现顺序排在 pprof 之后。
- **Pyroscope 适配器**：`POST /ingest?name=<app{labels}>&from=&until=&format=<pprof|folded|lines>`。`format=pprof` 复用 pprof 解码；`folded` / `lines` 为文本栈格式，单独小解析器；`name` 解析出 `service` 与标签。
- **OTLP Profiles 适配器**：解析 OpenTelemetry profiles proto（含规范化的 location/function/string lookup 表）。**该信号处于 Alpha、有 breaking change**——proto 版本钉选在 `src/protocol`，适配逻辑全部收敛在此适配器内，外部变动不外溢。该入口**默认开启**（无需 opt-in，与 upload / ingest 同级可用）；鉴于 Alpha 风险，仅保留一个配置开关（如 `MS_PROFILES_OTLP_ENABLED`）用于应急关闭，UI 对 OTLP 来源标注 experimental。

规范化后统一执行**双路落盘**：(1) 序列化为规范 pprof + zstd 写 object store；(2) 生成元数据 `RawEvent` 交 `IngestService::ingest`（`StreamType::Profiles`），从而自动获得 schema-on-write、配额门禁、保留。

**pipeline / masking 适用性**：profiles 元数据行可被 masking（对 label 值脱敏）；但 profiles 不作为 pipeline transform 的 target（栈语义不适合通用 transform），与 `Extend` 类似在 `allowed_as_pipeline_target` 上返回 `false`。

## Decision 3 — 火焰图聚合查询

`GET /api/v1/profiles/flamegraph?service=&type=&from=&to=&label=k:v&...`：

1. 用元数据流（parquet）按 service / type / label / 时间窗口检索命中的 `object_key` 集合；
2. 设窗口内 profile 数上限（如 `profiles.flamegraph.max_merge`，默认 1000），超限则按时间均匀采样并在响应标 `truncated: true`；
3. 拉取 blob、解码、**合并为聚合栈树**（按 frame 路径求和），输出 flamebearer（`names[]` + `levels[]`，与生态火焰图渲染兼容）；
4. 结果按 `(service,type,window,labels)` 指纹缓存（复用现有 caching 层）。

`GET /api/v1/profiles/diff?...&baselineFrom=&baselineTo=`：分别聚合 baseline 与 comparison 两棵树，按 frame 路径求差，输出带正负增量的 diff flamebearer。

`GET /api/v1/profiles`（列表）/ `GET /api/v1/profiles/{id}`（原始 pprof 下载）走元数据查询 + object store 取对象。

## Decision 4 — trace ↔ profile 关联

profile 样本常带 `trace_id` / `span_id` 标签（OTLP profiles 支持 link 到 span；Pyroscope span profiles 亦然）。摄取时把样本级 / profile 级的 trace 关联抽取进元数据行。由此：

- traces 详情页：对某 span 提供"查看该 span 期间火焰图"入口（按 `trace_id`/`span_id` + 时间窗口聚合）；
- profiles 浏览器：对带 trace 关联的 profile 提供跳转对应 trace 的入口。

不新建表，仅在 profiles 元数据流上存 `trace_id`/`span_id` 字段，关联在查询层完成。

## Decision 5 — 符号化（分阶段）

pprof / OTLP profiles 通常自带符号（function 表）。未符号化来源（如 eBPF 原生栈，仅含 `mapping.build_id` + 地址）MVP 阶段保留地址并标 `unsymbolized=true`，UI 显式提示。服务端符号化（上传 debuginfo / 对接 symbol server）作为后续**增强项**（建议 Pro 门禁），不在本变更范围。

## Decision 6 — 版本门禁（edition）

参考 `actions`（`edition: 'pro'` + `license.has_feature` + crate cfg-gate）的既有模式：

- **OSS**：三协议摄取、归档 + 元数据存储、单 profile 火焰图浏览、列表 / 筛选、trace 关联、按默认保留期留存。
- **Pro（门禁）**：差分火焰图（diff）、跨服务 / 大窗口聚合、长期保留、服务端符号化、Pyroscope `/render` 兼容出口。

上述门禁边界已确认采用（2026-06-18）；前端用既有 `FeatureGate` / `GatePage` 呈现价值与升级路径，不抛裸 403。

## Decision 7 — 配额与保留

profiles 摄取（解析后的元数据写入 + 归档 blob 字节）计入既有 per-org 配额（`max_ingest_qps`、`max_storage_bytes`），与 RUM replay 归档同样把对象字节计入 `max_storage_bytes`；超限返 `413`，不写对象。保留复用 stream `Retention`，到期同时清理 parquet 元数据与归档 blob。

## Module Layout（Rust）

遵循扁平布局偏好，profiles 后端逻辑集中在少数文件、避免多余 `mod.rs` 子目录包装：

- `crates/api/src/http/routes/profiles.rs`：三个摄取 handler + 查询 / 聚合端点。
- `crates/infra/src/profiles.rs`：`NormalizedProfile`、三协议适配器、pprof 编解码、归档、火焰图 / diff 合并（聚合算法若偏大，可拆 `profiles_merge.rs` 同层平铺，不建子目录）。
- `crates/domain/src/stream`：`StreamType::Profiles` 与 `allowed_as_pipeline_target`。
- `src/protocol`：钉选的 OTLP profiles + pprof `profile.proto` 生成产物。

## Risks & Mitigations

| 风险 | 缓解 |
|---|---|
| OTLP Profiles Alpha、breaking change | 适配层隔离 + 钉选 proto 版本；UI 标 experimental；入口默认开启，保留应急关闭开关 |
| 火焰图聚合读多 blob 慢 | 窗口内 profile 数上限 + 采样 + 指纹缓存 + 元数据预聚合（`total_value`） |
| 高基数栈存储成本 | zstd 归档 + 保留策略 + 采样率上限 |
| JFR 解析复杂 | 本变更内交付；先打通 pprof 再补 JFR，过渡期未实现路径返明确错误 |
| 未符号化栈影响可读性 | MVP 标注 `unsymbolized`，服务端符号化作为后续 Pro 增强 |

## Resolved Decisions

规划期（2026-06-18）已确认：

1. **OTLP Profiles 入口默认开启**——无需 opt-in，仅保留应急关闭开关（见 Decision 2）。
2. **火焰图聚合默认 `max_merge = 1000` + 窗口内均匀采样**——超限返 `truncated: true`，阈值后续按压测校准（见 Decision 3）。
3. **OSS / Pro 门禁采用 Decision 6 推荐边界**——OSS：三协议摄取、归档 + 元数据、单 profile 火焰图、列表 / 筛选、trace 关联、默认保留；Pro：diff、跨服务 / 大窗口聚合、长保留、服务端符号化、Pyroscope `/render` 出口。
4. **JFR 在本变更内交付**——实现顺序排在 pprof 之后（见 Decision 2 与 tasks 2.6）。

实现期仍需校准的参数：`max_merge` / 采样率的压测取值、Pro 长保留的具体周期。

## Phased Rollout（与 tasks.md 对应）

1. 数据模型与协议契约（StreamType、NormalizedProfile、proto 生成）
2. 摄取适配（pprof 直传 → Pyroscope → OTLP profiles）
3. 存储与落盘（blob 归档 + 元数据流 + 保留 / 配额）
4. 查询与聚合（列表 + flamegraph + diff + trace 关联）
5. 前端 Profiles 模块（IA、火焰图浏览器、列表、diff、关联、空状态、i18n）
6. 门禁与配额收口
7. 文档与本地全量验证
