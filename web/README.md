# MoleSignal Web

极简、键盘驱动的 SRE 调查工具。一个 `⌘K` 进入，靠键盘在 metric / trace / log 之间无限下钻，每一步都不丢上下文。

技术栈：**React 18 + Vite + TypeScript + shadcn/ui + Tailwind + uPlot + ReactFlow + TanStack Query + zustand**。

> 想了解整体设计取舍：见 `openspec/changes/web-investigation-shell/{proposal,design}.md`。
> 想看每条键位的来源：见 [`docs/web/keyboard.md`](../docs/web/keyboard.md)。

## 开发

```bash
# 在仓库根目录跑（pnpm workspaces 配置 → 根 package.json + pnpm-workspace.yaml）
pnpm install                          # 装所有 workspace 包
pnpm -C web dev                       # http://localhost:5173 — 代理 /api → http://localhost:5080（需自行起后端）
pnpm -C web dev:mock                  # 同上，但顺带在 5080 起 mock backend 给前端发确定性 JSON
pnpm -C web typecheck
pnpm -C web lint
pnpm -C web test                      # vitest
pnpm -C web playwright                # 端到端（含 a11y smoke）
pnpm -C web build                     # 产出 web/dist/
```

何时用哪个：
- `pnpm dev`：本地已经在 5080 跑 `cargo run --bin molesignal`。这是接真后端的开发流，登录走 `/api/v1/auth/signin`，org switcher、查询、dashboards 全部打真实数据。
- `pnpm dev:mock`：没有 Rust 工具链 / 后端临时 down。`scripts/dev-mock.ts` 复用 `playwright/fixtures/mockBackend.ts` 的 `registerRoutes`，独立起 express 在 5080。前端代码完全不变，vite proxy 把 `/api` 转过去。

## 目录约定

```
web/src/
├── main.tsx                 入口：Theme → Query → Tooltip → Keyboard → Router
├── routes/                  路由表 + RequireAuth + ShellRoot + 各页面
├── shell/
│   ├── AppShell.tsx         32px status strip + 52px hover IconRail + skip-to-content
│   ├── IconRail.tsx         hover 8px 热区展开
│   ├── StatusStrip.tsx      org / cluster / time / ⌘K / theme / avatar
│   ├── ThemeBootstrap.tsx   dark/light + compact/comfortable + prefers-color-scheme
│   ├── UrlHydration.ts      URL → store 同步反序列化
│   ├── tokens.css           9-color semantic palette（唯一色彩来源）
│   ├── fonts.css            Alibaba PuHuiTi 3.0 self-host
│   ├── lib/cn.ts            twMerge(clsx(...))
│   └── ui/                  shadcn 组件源码（18 个，全部 owned source）
├── palette/                 ⌘K：cmdk + 静态 action + 远端 search + fuzzy
├── keyboard/                全局 keydown 拦截器 + scope stack + help overlay
├── investigation/           调查栈：StackPortal + DrawerFrame + frame loaders + URL serializer
├── time/                    TimePicker + CursorChannel + halo
├── correlation/             8 条 link provider + server fallback + LinkMenu
├── viz/
│   ├── timeseries/          uPlot 自封装：LTTB + brush + cursor sync + axis modes
│   ├── trace/               canvas 火焰图 / 瀑布图 + d3-scale + hit test
│   ├── log/                 TanStack Virtual + NDJSON live tail + 字段染色
│   ├── topology/            ReactFlow + d3-force + DOI 着色 + edge 轮播
│   └── _demo/               1M 点 TimeSeriesPlot demo
├── stores/                  zustand：auth / useTimeStore / useKeyboardScope / useInvestigationStack
├── api/                     axios 客户端：每文件对应一组后端接口
├── lib/http.ts              axios instance（401 interceptor）
└── types/                   领域类型（手抄自后端 domain crate）
```

## 键位速查

| Key                | Action                                      |
| ------------------ | ------------------------------------------- |
| `⌘K`               | Command palette（输入即搜）                  |
| `Esc`              | 关闭当前 overlay / 弹一层调查抽屉             |
| `?`                | 键盘 help overlay                           |
| `⌘[` / `⌘]`        | 调查栈：后退 / 前进                          |
| `g s` / `g a` / `g d` / `g t` / `g l` | 跳路由（Services / Incidents / Dashboards / Investigate-traces / Investigate-logs） |
| `t`                | 时间窗 picker                               |
| `p`                | 钉住 / 解钉当前时间为锚                       |
| `y`                | 复制当前调查 URL（含 stack / time / anchor）  |
| `j` / `k`          | 列表：下 / 上一行                            |
| `J` / `K`          | 列表：下 / 上 10 行（仅 LogStream）           |
| `g g` / `G`        | 列表：顶 / 底（仅 LogStream）                 |
| `Enter`            | 激活当前行（展开 / 推 drawer）                |
| `/`                | 视图内搜索（trace flame / log stream）        |
| `n` / `N`          | trace flame：下 / 上一个搜索匹配              |
| `f` / `w`          | trace flame：flame / waterfall 模式          |

完整列表（含 scope-specific bindings）：`pnpm -C web dump:keymap`（生成 `docs/web/keyboard.md`）。

## 投资栈 URL 与 `y`

`y` 复制的链接是一段 base64 + pako 压缩的 JSON，含：

- 全局时间窗 `?time=from..to`
- 钉住锚 `?anchor=ISO`
- 调查栈每一帧 `?stack=<base64>`（最多 6 帧）

粘到新窗口（甚至发同事 Slack），打开后调查现场 1:1 还原。超过 4 KB 的 stack 会自动改用 `?blob=<uuid>` 走 backend `investigation_blobs` 表（7 天 TTL）。

## 主题与密度

```
<body data-theme="dark"   data-density="compact">       ← SRE 默认
<body data-theme="light"  data-density="comfortable">    ← 大屏 / 演示
```

切换：⌘K → `Toggle theme` / `Toggle density`，或者点 status strip 上的太阳 / 月亮图标。

色彩限制：**dark + light 各 9 色**（`bg / surface / primary / accent / red / green / yellow / blue / purple`），全部在 `shell/tokens.css` 单点定义。任何模块不允许引入额外色——viz canvas / uPlot 通过 `getComputedStyle(:root)` 读 var。

## 性能预算

| 场景                                | 目标         |
| ----------------------------------- | ----------- |
| 冷启动 → 首屏                       | < 1.2s（M1 笔记本） |
| ⌘K 首键 → palette 可输入             | < 60ms      |
| 10M 点 TimeSeriesPlot first paint    | < 80ms（已 downsample 到 chartWidth × 2） |
| 100k 节点 trace flame first paint    | < 200ms     |
| 1M 行 log scroll FPS                | ≥ 55        |
| 200 节点 service topology layout    | < 600ms     |
| Frame push（drawer 打开）            | < 100ms     |

CI 跑 `pnpm -C web playwright:perf` 验证。

## 测试

```bash
pnpm -C web test                       # vitest（unit + integration）
pnpm -C web test:coverage              # v8 coverage
pnpm -C web playwright                 # Playwright 端到端 + a11y
pnpm -C web playwright:perf            # 性能套件（不每次 CI 跑）
```

## 部署

```bash
# 容器内嵌（默认）：web 构建产物嵌入 crates/server/static → 一并 cargo build
docker build -t molesignal:dev -f deploy/docker/Dockerfile .

# 独立 nginx（split-pod 部署）：仅前端
docker build -t molesignal-web:dev -f deploy/docker/Dockerfile.web .

# 起 minio + postgres 依赖（dev）
bash scripts/dev-up.sh

# 起 backend
cargo run -p molesignal -- --config ./conf/config.toml
```

k8s manifests：`deploy/k8s/{30-router,40-ingester,50-querier,60-compactor,70-alert-manager,80-web}.yaml`。
