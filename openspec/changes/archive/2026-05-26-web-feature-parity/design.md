## Context

`/Users/gagral/code/openobserve` 是上游 reference 实现，前端 70+ 路由覆盖 RUM、Functions、IAM、多 Settings、Pipeline 编辑器等。molesignal 之前优先做 chrome / theming / 真后端接入，路由层只落了 17 个核心页。差距大到用户能直接观察到「很多功能看不到」。

后端能力大部分已经实装（见 `crates/api/src/http/routes/` 30+ 个 .rs 文件），缺的主要是前端壳子。`web-admin-pages`（之前开的、未实施的 change）覆盖 IAM 的 3 个子页（teams / groups / license），太窄不够用。

## Goals / Non-Goals

**Goals:**
- 一次性梳理出 openobserve vs molesignal 全量 sitemap 差集（**50+ 个路由**），落到 `tasks.md` 作为可执行清单
- 本 change 实施 P0 优先级（≈15 个路由，覆盖 RUM、Functions、Pipeline Editor、IAM 四大模块）
- 所有 P0 路由接已有的后端 endpoint，不阻塞后端实装
- Sidebar / SettingsMenu / i18n 同步扩展

**Non-Goals:**
- 不强求像素级复刻 openobserve 的视觉（molesignal 设计语言是「quiet canvas + 9 色 token」，与 openobserve Quasar 风格不一致）
- 不实施 P1 / P2（留作 follow-up change，避免单 PR 失控）
- 不重写已有 17 个路由（这些刚在 `web-backend-integration` / `web-theming-i18n` 里接通真后端）

## Decisions

### D1：sitemap diff 一次性产出，按优先级分 3 阶段

P0（本 change）= 完全缺的高价值模块 + 多数后端已就绪
P1（follow-up `web-feature-parity-settings`）= Settings 16 个子页
P2（follow-up `web-feature-parity-misc`）= 主页面二级路由扩展 + ingestion 实页 + short-url + actions

每阶段独立 propose / apply / archive，避免单 change 跨太多文件。

### D2：复用现有 admin 骨架而不是新写

`web-admin-pages` 提议过 admin 骨架（`web/src/admin/{PageHeader,DataTable,ConfirmDialog}.tsx`），但 change 没 apply 过。本 change 把骨架先落了，IAM 7 个子页全用它。

P1 / P2 也走这个骨架，让 50+ 页风格统一。

### D3：路由组织

```
web/src/routes/
├── rum/               # NEW (P0)
│   ├── Sessions.tsx + SessionDetail.tsx
│   ├── Errors.tsx + ErrorDetail.tsx
│   ├── Performance/{Overview,WebVitals,Errors,Apis}.tsx
│   └── SourceMaps.tsx + UploadSourceMaps.tsx
├── functions/         # NEW (P0)
│   ├── List.tsx + Edit.tsx
│   └── EnrichmentTables.tsx
├── pipelines/         # extend existing single-file
│   ├── Pipelines.tsx (existing list)
│   ├── Edit.tsx + Add.tsx + Import.tsx
│   └── History.tsx + Backfill.tsx
├── iam/               # NEW (P0)
│   ├── Users.tsx + ServiceAccounts.tsx + Organizations.tsx
│   ├── Groups.tsx + Roles.tsx + Quota.tsx
│   └── Invitations.tsx
└── ... (existing)
```

每个 P0 模块一个目录，避免顶层 `routes/` 文件爆炸。

### D4：sidebar 扩展但保持紧凑

```
OVERVIEW   Home
INGEST     Sources
OBSERVE    Logs, Metrics, Traces, RUM (NEW), Dashboards, Alerts
DATA       Streams, Pipelines, Functions (NEW), Reports
ADMIN      IAM (NEW), Actions (NEW, P2), Settings
```

3 个新顶层入口（RUM / Functions / IAM），ADMIN 组加 1 个。Actions 留 P2 时再加。

### D5：i18n 命名空间一次性扩

新增 5 个 namespace：`rum`、`functions`、`iam`、`actions`、`settings-admin`。每个 namespace 跟着 P0/P1/P2 阶段一起落。本 change 只落前 3 个（rum/functions/iam）。

### D6：后端缺口注释而不阻塞

pipeline history/backfill 后端端点缺。在前端代码注释 `// backend endpoint pending`，UI 显示「awaiting backend」空状态。后端补完后顺着同一份前端代码自动起来。

## Risks / Trade-offs

**[R1] 单 change 跨 15 路由仍然大**
→ Mitigation：tasks.md 分 4 模块，每模块单独可阶段性 commit；reviewer 按模块审

**[R2] 视觉/交互可能跟 openobserve 不一致**
→ Mitigation：本 change 只保证「功能可见」，视觉对齐留给后续 polish change

**[R3] Functions / Pipeline 这种「编辑器」类页面需要 VRL 高亮、cron 解析等专门组件**
→ Mitigation：第一版只做表单 + 文本框；可视化编辑器留 P2

**[R4] RUM 后端 `routes/rum.rs` 接口可能跟前端假设的 shape 不完全一致**
→ Mitigation：apply 阶段第一个 task 是「读 routes/rum.rs 全量 audit」，按真接口落 client / types
