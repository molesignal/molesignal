<!-- ppt-master-schema: design-spec/v1 -->
# MoleSignal Pitch en-US - Design Spec

## I. Project Information
| Item | Value |
| --- | --- |
| Project Name | MoleSignal Pitch en-US |
| Canvas Format | PPT 16:9 1280×720 |
| Page Count | 14 |
| Primary Language | en-US |
| Target Audience | Early-stage infrastructure investors |
| Communication Intent | Explain product evidence and market hypothesis, then request fundraising diligence |
| Desired Audience Outcome | Agree to a product demo and technical/commercial diligence |
| Core Message / Ask / Action | USD 750,000 to validate MoleSignal over 24 months |
| Delivery Context | Founder presentation and independent follow-up reading |
| Artifact Afterlife | Editable investor follow-up material |
| Reading Mode | balanced |
| Content Strategy | Preserve supplied facts, distinguish assumptions and targets |
| Design Style | Product evidence editorial; free design |
| AI Image Acquisition Path | not applicable |
| Generation Mode | continuous |
| Spec Refinement | disabled |
| Speaker Notes | enabled — delegated production choice |
| Custom Animations | disabled — delegated production choice |
| Narration Audio | disabled — delegated production choice |
| Created Date | 2026-11-02 |

## II. Canvas Specification
| Property | Value |
| --- | --- |
| Format | PPT 16:9 |
| Dimensions | 1280 × 720 |
| viewBox | `0 0 1280 720` |
| Margins | 56 px |
| Content Area | 1168 × 608 |

## III. Visual Theme
### Theme Style
- **Mode**: custom
- **Mode Behavior**: Preserve the requested topic order, expand product proof, close with budget and measurable proposed milestones.
- **Visual style**: custom
- **Visual Style Behavior**: A product-evidence editorial system: blue-green section labels, large navy titles, quiet light backgrounds, open text columns and full-width real screenshots; darker cover and closing pages frame the ask. Independent arguments use short titled paragraphs rather than dashboard-style cards.
- **Theme**: MoleSignal blue and teal
- **Tone**: Candid, technical, investor-readable
### Color Scheme
| Role | HEX | Purpose |
| --- | --- | --- |
| Background | #F7F9FC | Main field |
| Secondary background | #E8F2F4 | Selected supporting field |
| Primary | #102C44 | Titles and anchor pages |
| Accent | #007E87 | Important labels |
| Secondary accent | #1766D3 | Product context |
| Body text | #173247 | Reading |
| Secondary text | #53677B | Qualification |
| Divider | #D4DFE7 | Separation |

## IV. Typography System
### Font Plan
| Role | Character (Reference) | Primary | English if non-English | Fallback tail |
| --- | --- | --- | --- | --- |
| Title | Clear bold sans | Arial | Arial | sans-serif |
| Body | Readable sans | Arial | Arial | sans-serif |
- **Title stack**: Arial
- **Body stack**: Arial
### Font Size Hierarchy
| Purpose | Anchor Size (px) |
| --- | ---: |
| Body | 24 |
| Title | 44 |
| Subtitle | 30 |
| Annotation | 20 |
| Footnote | 16 |
| Display | 76 |
| Kicker | 18 |

## V. Layout Principles
### Deck-wide Direction
- **Hierarchy direction**: Title, primary evidence, implication, qualification.
- **Composition tendency**: Large editorial field; screenshots dominate product evidence pages.
- **Cross-page continuity**: Repeated brand line and page index; vary density with the page job.
- **Spacing posture**: Open, with denser competitor and budget pages.
- **Spacing anchors**: page margin 56; block gap 32; column gutter 48; corner radius 0; body leading 36.

## VI. Icon Usage Specification
- **Primary bundled library**: none
| Icon Path | Suitable Scenarios |
| --- | --- |

## VIII. Image Resource List
| Filename | Dimensions | Ratio | Purpose | Type | Image pattern | Crop Policy | Acquire Via | Status | Reference | text_policy | page_role |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| logs.png | 1280×720 | 16:9 | Product log explorer proof | Screenshot | Wide product figure with separate caption | no-crop | user | Existing | Repository Playwright fixture screenshot; demo data | embedded | local |
| traces.png | 1280×720 | 16:9 | Product trace explorer proof | Screenshot | Wide product figure with separate caption | no-crop | user | Existing | Repository Playwright fixture screenshot; demo data | embedded | local |

## IX. Content Outline
### Part 1: Investment case

#### Slide 01 - MoleSignal
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: MoleSignal
- **Core message**: 开源、自部署的可观测性与事件响应。融资75万美元，24个月；发布前研发。
- **Content**: 开源、自部署的可观测性与事件响应。融资75万美元，24个月；发布前研发。

#### Slide 02 - Customer pain
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Customer pain
- **Core message**: 排障上下文分散；组件维护与总成本；数据及模型控制。是目标客户假设，不代表全部工具。
- **Content**: 排障上下文分散；组件维护与总成本；数据及模型控制。是目标客户假设，不代表全部工具。

#### Slide 03 - Product approach
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Product approach
- **Core message**: 统一遥测、事件流程、权限和模型选择。OTel/Prometheus接入；Rust、PostgreSQL、对象存储。
- **Content**: 统一遥测、事件流程、权限和模型选择。OTel/Prometheus接入；Rust、PostgreSQL、对象存储。

#### Slide 04 - Log investigation
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Wide screenshot evidence.
- **Title**: Log investigation
- **Core message**: 真实日志界面；SQL、字段筛选、时间分布、trace_id；测试演示数据。
- **Content**: 真实日志界面；SQL、字段筛选、时间分布、trace_id；测试演示数据。
- **Images**: logs.png; complete screenshot and demo-data caption.

#### Slide 05 - Trace investigation
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Wide screenshot evidence.
- **Title**: Trace investigation
- **Core message**: 真实链路界面；延迟、错误、服务筛选；跨信号调查；测试演示数据。
- **Content**: 真实链路界面；延迟、错误、服务筛选；跨信号调查；测试演示数据。
- **Images**: traces.png; complete screenshot and demo-data caption.

#### Slide 06 - Incident response & Mole Agent
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Incident response & Mole Agent
- **Core message**: 值班、升级、状态页与复盘；Agent记录证据与假设，模型可选，风险操作审批。
- **Content**: 值班、升级、状态页与复盘；Agent记录证据与假设，模型可选，风险操作审批。

#### Slide 07 - Target customers
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Target customers
- **Core message**: 使用者SRE/平台/后端；买单者CTO/工程负责人；初期聚焦有限平台人力且需收拢响应流程的团队。
- **Content**: 使用者SRE/平台/后端；买单者CTO/工程负责人；初期聚焦有限平台人力且需收拢响应流程的团队。

#### Slide 08 - Competitive landscape
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Competitive landscape
- **Core message**: Datadog完整SaaS与多维计费；Grafana组件生态与Cloud IRM；OpenObserve统一自部署/Cloud、AI与事件。
- **Content**: Datadog完整SaaS与多维计费；Grafana组件生态与Cloud IRM；OpenObserve统一自部署/Cloud、AI与事件。
- **Fact IDs**: F001, F002, F003, F004, F005, F006, F007, F008, F009

#### Slide 09 - Why MoleSignal
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Why MoleSignal
- **Core message**: Apache2.0、自部署、模型与权限可控；从现有告警渐进接入；以具体体验验证差异，不宣称独有或性能优势。
- **Content**: Apache2.0、自部署、模型与权限可控；从现有告警渐进接入；以具体体验验证差异，不宣称独有或性能优势。

#### Slide 10 - Business model
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Business model
- **Core message**: 开源自部署免费；Cloud订阅拟含额度加用量、价格待验证；Cloud未上线；付费价值托管、升级、备份、支持。
- **Content**: 开源自部署免费；Cloud订阅拟含额度加用量、价格待验证；Cloud未上线；付费价值托管、升级、备份、支持。

#### Slide 11 - Current stage
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Current stage
- **Core message**: 发布前打磨；外部用户0、付费客户0；公司未注册；已有源码/前端/文档，待外部验证可靠性与付费。
- **Content**: 发布前打磨；外部用户0、付费客户0；公司未注册；已有源码/前端/文档，待外部验证可靠性与付费。

#### Slide 12 - Founder
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Founder
- **Core message**: 许宗盛多年Rust/Go云原生与交易系统经验；DEX撮合、清结算重写；独立全栈交付，AI工具辅助。
- **Content**: 许宗盛多年Rust/Go云原生与交易系统经验；DEX撮合、清结算重写；独立全栈交付，AI工具辅助。

#### Slide 13 - Funding plan
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: Funding plan
- **Core message**: USD750000/24个月；建议55%研发412500、20%Cloud150000、15%客户及市场112500、10%设立法务及缓冲75000；月均31250含缓冲，非现有burn。
- **Content**: USD750000/24个月；建议55%研发412500、20%Cloud150000、15%客户及市场112500、10%设立法务及缓冲75000；月均31250含缓冲，非现有burn。
- **Data class**: scenario

#### Slide 14 - 24-month validation plan
- **Audience move**: Understand this aspect of the investment case and its evidence boundary.
- **Relationships**: none; grouping and ordering are expressed in the prose.
- **Composition**: Editorial text with deliberate hierarchy.
- **Title**: 24-month validation plan
- **Core message**: 0–6月发布与3–5设计伙伴；7–12月Cloud私测与3付费团队；13–24月争取10付费团队并验证留存；均为目标非业绩。邀请产品演示、技术尽调。
- **Content**: 0–6月发布与3–5设计伙伴；7–12月Cloud私测与3付费团队；13–24月争取10付费团队并验证留存；均为目标非业绩。邀请产品演示、技术尽调。
- **Data class**: scenario

## X. Speaker Notes Requirements
- **Generation**: enabled
- **Filename**: match each SVG filename under notes/
- **Content**: Explain the final visible page, qualify forecasts, include local evidence and official competitor source URLs.
- **Total duration**: 10–12 minutes
- **Notes style**: Conversational and candid
- **Presentation purpose**: Product demonstration and investment diligence
