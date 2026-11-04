## Research Brief

- 基线：MoleSignal README.zh-CN.md 描述自托管遥测、事件响应和 Mole Agent；用户明确产品仍处于研发阶段，尚无客户。
- 研究缺口：Datadog、Grafana、OpenObserve 当前官方产品能力、部署选择及商业模式，供公平竞品对比使用。
- 受众与意图：早期投资人；形成中英文两份融资介绍，区分已实现能力、客户验证和未来计划。
- 范围：仅采用已打开的官方页面，不扩展市场规模、估值、竞品成本倍数或未经独立验证的性能宣传。
- 检索日期：2026-11-02。网页能力描述代表厂商公开表述，未实际部署测试竞品。

## Datadog

- [F001] Datadog 将监控、呼叫通知和值班事件管理纳入同一事件响应流程，并提供状态更新和复盘能力。

- [F002] Datadog 的 Bits Investigation 提供基于遥测的自动化调查；因此 AI 调查不是 MoleSignal 独有的产品类别。

- [F003] Datadog 分产品计费：基础设施按主机/月，日志包含摄入量与索引事件量等计费维度；不能将其概括成单一按 GB 价格。

## Grafana

- [F004] Grafana 开源产品组合包含可视化平台、日志 Loki、指标 Mimir 和链路 Tempo；它是多个专用项目组成的生态。

- [F005] Grafana Cloud IRM 提供值班、告警路由与升级、事件协作和 AI 调查，IRM 按月活跃用户计费。

## OpenObserve

- [F006] OpenObserve 官方平台包括统一日志、指标、链路、RUM，以及 Incidents、Workflows、SLOs、AI SRE 和 AI Assistant。

- [F007] OpenObserve 支持单二进制或 Kubernetes 自部署，也提供 Cloud；其技术基础包括 Rust、DataFusion、Parquet 和对象存储。

- [F008] OpenObserve 提供免费开源自部署、付费自部署企业版和 Cloud；官方列明开源核心采用 AGPL-3.0。

- [F009] OpenObserve Cloud 以摄入 GB 为主要计费维度，用户不限量且无按席位收费；保留期延长等可以产生额外费用。

## 材料冲突与证据边界

- [F001, F002, F005, F006] 三家竞品均已有事件响应或 AI 调查相关能力。任何“竞品只有看板”“竞品没有 AI”“只有 MoleSignal 打通观测与响应”的表述均不受这些官方来源支持。
- [F006, F007] Rust、统一遥测、对象存储、单二进制、自部署、AI 和事件管理均与 OpenObserve 存在重合，不能单独作为排他性壁垒。
- [F003, F009] 本次仅核实计费维度，未建立同负载、同保留期、同服务等级的总成本模型，因此不能声称 MoleSignal 已便宜某个倍数。
- 没有运行竞品，无法断言其功能质量、易用性或生产可靠性差于 MoleSignal；这些比较需要试用或设计伙伴验证。
- 本次不对 Datadog 全平台可否私有化、Grafana Assistant 所有自管理部署限制，以及 OpenObserve 每项能力的版本授权边界作未核实的推断。
