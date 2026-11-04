# MoleSignal 融资材料事实底稿

核实日期：2026-11-02。交付需求：中文、英文各一份 Pitch Deck。

## 已知事实
- 用户明确：产品研发阶段，尚无客户。不得将功能、测试数据或规划写成客户验证、收入、增长或生产 SLA。
- README.zh-CN.md：定位为可完全自托管的可观测性与事件响应平台，包含遥测、告警、值班、状态页和 Mole Agent。
- LICENSE：Apache License 2.0。
- 当前 README 描述 SQL/PromQL、OpenTelemetry、跨信号调查、内置权限与审批、模型可选、Rust 单二进制和 PostgreSQL + 本地/对象存储。表示代码与文档能力，不证明大规模生产性能。
- 网站 messages/zh.json cloud：Cloud 候补，优先自托管 v1.0，无公开上线日期；BYOC 和多租户托管在评估。
- 网站 pricing：Cloud 按 GB 计费待定。Deck 主模式遵从用户：开源自部署 + Cloud 订阅。可将订阅含额度、超额按用量作为建议，而非已发布价格。

## 口径冲突与边界
- 网站 FAQ 关于 SSO/审计收费的表述与 README 内置治理表述不完全一致。Deck 不做功能付费墙承诺，聚焦托管服务价值。
- 官网关于 SOC2/HIPAA 的规划不能写成已获认证；无认证证据，不使用认证徽章。
- 不写节省百分比、性能倍数、TAM、ARR、客户 Logo，除非有进一步可靠数据。
- 用户创始人履历、融资金额、币种、覆盖周期待补充。不得用仓库提交量推断年限或任职经历。

## 截图来源候选
- web/playwright/tests/05-visual.spec.ts-snapshots/apm-overview-light-comfortable-chromium-darwin.png
- web/playwright/tests/05-visual.spec.ts-snapshots/logs-light-comfortable-chromium-darwin.png
- web/playwright/tests/05-visual.spec.ts-snapshots/traces-light-comfortable-chromium-darwin.png
- web/playwright/tests/05-visual.spec.ts 明确后端 mocked：真实前端界面，测试演示数据。采用时需在中英文版本标注。

## 待验证商业假设
- 初始使用者：SRE、平台工程师、后端负责人；买单者：CTO、工程负责人或平台预算负责人。
- 优先切入：已有开源监控、有限平台人力、需要收拢事件响应的小中型工程组织。
- 获客方式建议：从现有告警接入与设计伙伴试点开始，验证安装成功、持续使用、响应流程覆盖和付费意愿。
- 融资应与产品可靠性、设计伙伴、Cloud 私测及首批付费转化里程碑绑定；具体金额不冒充创始人决定。

## 已读本地依据
- /Users/gagral/code/molesignal/molesignal/README.zh-CN.md
- /Users/gagral/code/molesignal/molesignal/LICENSE
- /Users/gagral/code/molesignal/molesignal-website/messages/zh.json
- /Users/gagral/code/molesignal/molesignal/web/playwright/tests/05-visual.spec.ts
