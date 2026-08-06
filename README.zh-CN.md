<div align="center">

# MoleSignal

**在不丢失上下文的情况下，从指标到追踪再到日志进行关联。**

自托管，OpenTelemetry 原生。三种信号共享同一存储层与同一查询引擎，
所以一条 trace、它对应的日志、同一分钟的主机 metric 是*真的*串在一起的——不是靠你手动 copy-paste 拼出来的。

[为什么是 MoleSignal](#为什么是-molesignal) · [快速开始](#快速开始) · [功能](#功能) · [架构](#架构) · [现状](#现状) · [English](README.md)

</div>

---

## 为什么是 MoleSignal

现有可观测工具逼你二选一：

- **商业 SaaS**（Datadog / New Relic / Splunk）—— 三种信号确实串得通，但账单跟流量线性增长。一个中等规模的团队，100 GB/天 一个月轻松 **2k–10k 美元**；想省钱只能砍 ingest，砍 ingest 又看不到东西。
- **开源拼装**（Loki + Mimir + Tempo + Grafana，或 ELK + Prometheus + Jaeger）—— 不要钱，但**日志 / 指标 / trace 在三个独立的存储里、用三种不同的查询语言**。排障时人人都需要的"trace ↔ log ↔ 主机 metric"那一跳必须人肉拼：复制一个 trace_id、切 tab、粘进去、再粘一次时间范围、祈祷两边时钟对得上。

MoleSignal 走第三条路：**一个存储层（对象存储上的 Parquet）+ 一个查询引擎（DataFusion + Arrow）+ 一个元数据层（Postgres）**——三种信号在**数据层**就是串通的，不是靠 dashboard 拼出来的。自托管，所以你的账单就是 S3 的成本。

|  | 商业 SaaS<br>(Datadog / New Relic) | 开源拼装<br>(Loki + Mimir + Tempo) | **molesignal** |
|---|---|---|---|
| 100 GB/天成本 | ~$2k+/月（线性涨） | 仅基础设施 | **仅基础设施** |
| 三种信号同存储 | ✅（他们的云） | ❌ 3 个 store、3 种查询语言 | **✅ Parquet + DataFusion** |
| 跨信号关联 | ✅（付费） | ⚠️ 手动 copy-paste trace_id | **✅ 原生（`/web/correlation/*`）** |
| 数据归属 | 他们的云 | 自托管 | **自托管** |
| 起步时间 | 5 分钟（配 agent） | 6 小时+（5 个组件 + Grafana） | **`docker compose up` 一行** |
| OpenTelemetry 原生 | 是 | 部分 | **是（10 个采集协议）** |
| 实时告警（<1s） | 是 | 否（评估周期 ≥ 抓取周期） | **是（`kind: realtime`）** |
| 原生多租户 | 是（按账户） | 否 | **是（planner 层 org rewrite）** |

> **状态：** 早期项目，**尚未正式发布**。目前 pre-1.0，欢迎首批贡献者与设计伙伴。详见 [现状](#现状)。

---

## 快速开始

```bash
git clone https://github.com/molesignal/molesignal
cd molesignal

# 一行启动沙盒（Postgres + MinIO + molesignal standalone）
docker compose -f deploy/docker/docker-compose.yaml --profile standalone up

# UI：       http://localhost:5080
# S3 控制台： http://localhost:9001  (minioadmin / minioadmin)
```

发送第一条数据：

```bash
# OTLP HTTP（OpenTelemetry Collector / SDK / Vector / Fluent Bit 直接对接）
curl -X POST http://localhost:5080/api/v1/ingest/logs/app \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer <jwt>' \
  -d '[{"_timestamp":1700000000000000,"level":"error","msg":"db pool exhausted","trace_id":"abc123"}]'

# 查询 —— 注意同一个 trace_id 把日志、trace 和主机 metric 串起来
curl -X POST http://localhost:5080/api/v1/query \
  -H 'authorization: Bearer <jwt>' \
  -H 'content-type: application/json' \
  -d '{"org_id":"<from-login>","language":"sql",
       "statement":"SELECT * FROM app WHERE trace_id = '\''abc123'\''",
       "time_range":{"start":0,"end":2000000000000000},
       "stream":{"name":"app","stream_type":"logs"}}'
```

Vector / Fluent Bit / OTel Collector / Prometheus remote_write 等完整对接示例见 [docs/integrations.md](docs/integrations.md)。

---

## 功能

### 🔗 跨信号关联（杀手功能）

一条 trace、它的日志、同一分钟的主机 metric **共享同一存储、同一时间索引、同一租户范围**。再也不用"复制 trace_id、切 tab、粘、对时间"：

- `GET /api/v1/web/correlation/{from_kind}/{to_kind}` —— 服务端跨信号 join，自动 prefill 过滤条件
- 从 trace 自动派生 Service graph + RED metrics，可直接用作拓扑视图
- 时间锚点同步所有面板（一次 zoom 自动广播）
- 调查栈：从 `metric → trace → log → host` 来回钻而不丢上下文

### 📡 采集（10 个协议，drop-in 替代）

| 协议 | Endpoint | 直接替代 |
|---|---|---|
| OTLP gRPC | `:5082` | OpenTelemetry SDK / Collector |
| OTLP HTTP | `POST /api/v1/{logs,metrics,traces}` | OTel HTTP exporter |
| Prometheus remote_write | `POST /api/v1/prometheus/api/v1/write` | Prometheus / VictoriaMetrics |
| Elasticsearch `_bulk` | `POST /api/v1/_bulk` | Filebeat / Vector ES sink / Logstash |
| Loki push | `POST /api/v1/loki/api/v1/push` | Promtail / Vector Loki sink |
| Syslog UDP/TCP | `[syslog].udp_bind` / `tcp_bind` | rsyslog / syslog-ng |
| Kinesis Firehose | `POST /api/v1/_kinesis_firehose` | AWS Firehose |
| Cloudflare Logpush | `POST /api/v1/_cloudflare` | Cloudflare Logpush |
| Heroku log drain | `POST /api/v1/_heroku` | Heroku |
| 原生 HTTP JSON | `POST /api/v1/ingest/{type}/:stream` | curl / 应用 SDK |

### 🌐 RUM & APM

- **RUM** — Datadog 兼容接收端，支持 session / action / error / replay；JavaScript 与 native stack 的 sourcemap 符号化
- **APM** — 从 trace 派生 service graph、RED metrics 与依赖视图；service/endpoint 逐级下钻

### 🗃️ 存储与查询 — 一个引擎搞定全部

- **列式存储** —— Parquet on S3 / GCS / Azure / MinIO；Postgres 存元数据
- **Tantivy 倒排索引** —— 查询时文件级裁剪（典型 ~99% 减少扫描）
- **查询引擎** —— 完整 SQL，含 join / CTE / window function，跨 logs / metrics / traces
- **PromQL 子集** —— `rate` / `increase` / `sum/avg/min/max/count by/without` / `histogram_quantile`（[路线图](docs/promql_subset.md)）
- **Arrow Flight 分布式查询** —— coordinator 按一致性哈希分片，peer 流式回传 `RecordBatch`
- **3 级缓存** —— `parquet_file_meta` / `parquet_meta` / `query_result`，外加默认开启的 parquet 磁盘缓存（`./data/cache/parquet`，10 GB LRU；通过 `[cache.disk_cache]` 调整或关闭）
- **ParquetFileMeta 冷分层** —— 超过 `[storage.parquet_file_meta_dump].cold_after_days`（默认 30 天）的分区被序列化下沉到 object_store，主元数据表始终保持小；查询路径自动跨冷热合并

### 📊 仪表盘

- 自定义仪表盘，支持图表变量，time-series / stat / table / topology 等面板类型
- Dashboard contracts 支持版本化部署与同步

### 🌍 联邦搜索

- 跨集群资源同步（CloudEvents 1.0）
- Dashboards、告警规则、regex patterns 跨集群共享，Lamport 版本号做冲突解决

### 🚨 告警

- **三种规则类型**：`scheduled`（周期 SQL eval）/ `realtime`（入口谓词匹配，<1s 触发）/ `anomaly`（MAD 基线 + 3σ）
- **升级策略** —— 多步 + ack 超时 + 排班轮值 + override
- **Notify 管理** —— 加密 Connector、用户 Endpoint/Preference、策略匹配、三级兜底、确认升级与幂等投递

### 🛡️ 平台

- **SSO** — OIDC / SAML / LDAP，支持身份字段映射与用户组角色绑定
- **RBAC** — API token（`ms_<prefix>_<secret>`）与 JWT 并存；per-token 角色、过期、last-used
- **多租户** — planner 层 `org_id` 强制 rewrite，跨 org 数据零泄漏可能
- **审计日志** — 覆盖所有写操作
- **字段级加密** — AES-256-GCM + cipher root key envelope；VRL `encrypt()` / `decrypt()` 内置
- **per-org 配额** — ingest QPS / query QPS / 存储 cap

### 🤖 Mole Intelligence

- 基于遥测数据的自然语言对话（SSE 流式）
- MCP server 对接 AI 助手
- 按 org 配置 model provider / toolset / prompt
- 从对话生成仪表盘草稿

### ⌨️ 键盘友好的 Web UI

- ⌘K 命令面板 —— 所有 stream / dashboard / 告警 / saved view 一键直达
- 调查栈（最多 6 帧）—— 下钻时压栈；`⌘[` / `⌘]` 前后切；pin 帧锁定上下文
- 任何可点击操作都可键盘到达

### 🧩 Pipeline 函数（VRL + 可选 JS + LLM）

函数是挂在 pipeline 步骤上的可复用转换逻辑。有三种类型，运行在 ingest 热路径上：

- **VRL** — 始终可用。按 `(function_id, updated_at)` 编译，基于上游 `vrl::compiler` stdlib（`del` / `parse_json` / `to_int` / `match` / `encrypt` / `decrypt` 等）。
- **JavaScript** — 可选，基于 `deno_core`（V8）。默认关闭，因为引入 `deno_core` 会将干净构建从 ~1.5 分钟拉到 ~5 分钟。通过编译时 feature `--features js-runtime` 开启（无运行时开关——二进制要么带 V8 要么不带）。feature 关闭时，JS 函数 POST 返回 `400 javascript runtime not enabled`。
- **LLM** — 可选，将事件 JSON 交给配置好的 AI provider（intelligence）评估，模型输出写回事件的可配置字段（默认 `_llm_eval`）。由运行时开关控制：

  ```toml
  [functions]
  llm_eval_enabled = true
  ```

  关闭时 pipeline 拒绝 `language=llm` 的步骤。

### ☸️ 运维

- **6 个无状态 role** —— `router` / `ingester(SF + PVC)` / `querier` / `compactor` / `alert-manager` / `connector`，只 ingester 有本地状态（WAL，≤ flush_interval 窗口）
- **单二进制** —— 同一镜像跑所有 role，靠 `MS_NODE_ROLES` 区分
- **Kubernetes manifest** 见 [deploy/k8s/](deploy/k8s/)，Docker Compose 提供 `standalone` 与 `multirole` 双 profile
- **Prometheus `/metrics`** 默认暴露，含 cache / object_store / ingester / compactor 各层指标
- **健康探针** —— readiness 由 ingester WAL replay 完成 + object_store round-trip 探活共同决定

---

## 架构

```
                          ┌──────────┐
   OTel / Vector / ...  ─►│  router  │─► 一致性哈希(org,stream) ─► ingester(s)
                          └──────────┘                              │
                               │                                    ▼
                               ▼                            WAL + Arrow buffer
                       /api/v1/{ingest,query,...}                   │
                               │                          flush → Parquet + Tantivy
                               ▼                          上传 S3
                       ┌──────────────┐                             │
                       │   web shell  │                             ▼
                       │ (⌘K + 调查栈) │       ┌────────────────────────────┐
                       └──────┬───────┘       │ ParquetFileMeta in Postgres       │
                              │ /query        │ object_store in S3/GCS/... │
                              ▼               └────────────────────────────┘
                       ┌──────────────┐                  ▲
                       │  querier(s)  │── Arrow Flight ──┘
                       └──────────────┘    do_get（按哈希分片分布式扫描）
```

**关键点：** logs、metrics、traces 全部落到**同一批** Parquet 文件里（不同 stream，相同物理存储）。一条 SQL 查询可以原生 join 三种信号——不需要跨 store 联邦，不需要人肉对 trace_id。

---

## 现状

Pre-1.0，**早期项目**。发布日期 YYYY-MM-DD。

| 领域 | 状态 |
|---|---|
| 采集链路（WAL + buffer + flush） | ✅ 已通 |
| 10 个采集协议 | ✅ 已通 |
| Arrow Flight 分布式查询 | ✅ 已通 |
| 3 级缓存 + 磁盘缓存 | ✅ 已通 |
| 多租户 planner rewrite | ✅ 已通 |
| realtime + scheduled + anomaly 告警 | ✅ 已通 |
| Cipher keys + audit + quotas | ✅ 已通 |
| 跨信号关联 API | ✅ 已通 |
| Web shell（⌘K + 调查栈） | ✅ 已通 |
| SSO（OIDC / SAML / LDAP） | ✅ 已通 |
| 首启动 demo 数据集 | ⏳ 待做 |
| 生产硬化 | ⏳ 需真实负载验证 |

**如果你试用了，请 [开 issue](https://github.com/molesignal/molesignal/issues) —— 每一条反馈都会影响下个版本。** 我们特别关心：安装阻力、协议字段缺失、跨信号关联的缺口。

---

## 编译

```bash
# 开源生产制品
BUILD_ID=local-001 cargo build --release --locked -p molesignal

# 付费版（需 SSH key 拉私有仓 git@github.com:molesignal/molesignal-.git）
BUILD_ID=local-001 cargo build --release --locked -p molesignal --features <features>

# 晋升时只修改运行时部署元数据，复用同一个二进制。
RELEASE_CHANNEL=alpha ./target/release/molesignal --config conf/config.toml
```

所有可交付制品统一使用 Cargo `release` profile。`BUILD_ID` 与 Git SHA 标识构建制品；运行时 `RELEASE_CHANNEL`（`alpha`、`beta`、`rc`、`stable`）表示部署成熟度。通道晋升复用同一个二进制或不可变镜像，不重新编译。

---

## 参与贡献

欢迎 PR —— 从 `openspec/changes/*/tasks.md` 开始读。约定：

- DDD 分层：不要把 infra 关注点塞进 `domain/`
- 每个 public 类型用一句 doc comment 说明*为什么*存在
- 集成测试放在 `tests/*_it_*.rs`；依赖 Docker 的用 `MS_RUN_IT=1` 门控
- push 前跑 `cargo fmt --all` + `cargo clippy --workspace --all-targets`

Issue / RFC / 设计讨论都在 GitHub 上。Discord / Slack 暂未建，等第一批用户到位后再开。

### 贡献者

感谢所有为 MoleSignal 做出贡献的人：

<a href="https://github.com/molesignal/molesignal/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=molesignal/molesignal" alt="Contributors" />
</a>

---

## 许可证

Copyright 2026 MoleSignal Authors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
