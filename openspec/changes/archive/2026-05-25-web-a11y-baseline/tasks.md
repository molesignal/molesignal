## 1. 对比度自动检查

- [x] 1.1 `web/scripts/check-contrast.ts`：解析 `tokens.css` 出 dark + light 各 9 色 + bg/surface/fg variant；调 `wcag-contrast::hex` 算 ratio
- [x] 1.2 配 AA 阈值（4.5:1 body / 3:1 large/UI）；fail 时按格式 `FAIL <theme>.<fg> ON <theme>.<bg>: X.XX:1 < Y:1` 输出
- [x] 1.3 `web/package.json` scripts 加 `"a11y:contrast": "tsx scripts/check-contrast.ts"`
- [x] 1.4 deps：`pnpm add -D wcag-contrast`
- [x] 1.5 跑一次：列出当前所有 fail pair（不修，留给 `web-ui-polish`）—— 5 个 light theme 失败（tx-2/red/green/yellow/orange on bg-0）已写入 `scripts/check-contrast.baseline.json`

## 2. axe-core 全路径扫

- [x] 2.1 `web/playwright/tests/a11y-routes.spec.ts`：遍历 11 条认证路由
- [x] 2.2 每条用 mockBackend fixture（offline + page.route）保确定性
- [x] 2.3 用 `new AxeBuilder({ page }).analyze()`；assert critical = 0
- [x] 2.4 moderate / minor 通过 `console.log(JSON.stringify(...))` 输出到 report

## 3. Focus ring 视觉 baseline

- [x] 3.1 `web/playwright/tests/a11y-focus-ring.spec.ts`：4 viz route × 4 combo = 16 test
- [x] 3.2 每 test：navigate → Tab 进焦点 → `locator.screenshot({ padding: 8 })` → `toMatchSnapshot()`（实装用 `expect(locator).toHaveScreenshot()` 配 `maxDiffPixelRatio: 0.005`）
- [x] 3.3 第一次跑 `--update-snapshots` 生 16 PNG baselines
- [x] 3.4 第二次跑（不带 update）验证稳定

## 4. 键盘地图测试

- [x] 4.1 `web/playwright/tests/a11y-keyboard-map.spec.ts`：`import { GLOBAL_KEYMAP }`；`for (const b of GLOBAL_KEYMAP) test(...)`
- [x] 4.2 每 binding：fixture offline 进 `/investigate` → `page.keyboard.press(b.keys)` → assert 该 binding `description` 对应的 DOM 状态
- [x] 4.3 chord binding（`g s` 等）按 `keys.split(' ')` 顺序发送

## 5. CI 集成

- [x] 5.1 `web.yml` ci job 加 `pnpm -C web a11y:contrast`
- [x] 5.2 playwright job 自动跑新的 3 个 a11y spec（无需配置 grep，靠 `testMatch=/.*\.spec\.ts$/` + 文件落在 `playwright/tests/` 即可）
- [x] 5.3 失败时 trace artefact 上传含 a11y spec 的 report（继承自 web-playwright-runtime 的 yml）

## 6. 完工校验

- [x] 6.1 `pnpm -C web a11y:contrast` 跑出 baseline 报告（含失败 pair 列表，留给后续）—— `37 pass, 5 known-pre-existing (allowlisted), 0 new failure(s)`
- [x] 6.2 `pnpm -C web playwright test playwright/tests/a11y-*.spec.ts` 全绿 —— 11 routes + 16 binding + 16 focus-ring = 43 a11y case
- [x] 6.3 16 张 focus ring PNG 提交在 `a11y-focus-ring.spec.ts-snapshots/`
- [x] 6.4 `openspec validate web-a11y-baseline --type change --strict` 通过

## 7. 实施期间发现并修齐的同源债务（tasks.md 起草时未预料）

> 没修这些就达不到 6.2 `critical = 0` 的硬指标 —— 一并完成以闭环。

### 7.1 contrast 脚本 baseline 机制
- [x] 7.1.1 spec 严格说"fail 即 exit 非 0"，但 design D5 又说"本 change 不修，留给 web-ui-polish"。两者只能靠"已知失败 allowlist"调和：增 `scripts/check-contrast.baseline.json`，列出 5 个已知 light-theme fail，脚本对 baseline 内的 fail 报 WARN 不 exit；任何新增 fail（不在 baseline 内）才 exit 非 0
- [x] 7.1.2 脚本同时报告"已不再失败的 baseline 条目"，提醒 web-ui-polish 修完后顺手缩 baseline

### 7.2 ts/lint 配套
- [x] 7.2.1 `wcag-contrast` 无 d.ts → 加 `scripts/wcag-contrast.d.ts` ambient 模块声明
- [x] 7.2.2 ESLint globals 补 `process`（contrast 脚本用 `process.exit/process.stderr`）

### 7.3 axe critical 真实违规（不在 design 预料范围）
> design D5 只说对比度留给后续；axe critical 是硬指标。修齐发现的 6 处违规以达到 spec scenario "all routes report critical=0"。
- [x] 7.3.1 `src/routes/Login.tsx` Field 组件 `<label>` 不包 input，仅做 sibling → 改 `<label>` 包裹 + `aria-label`
- [x] 7.3.2 `src/routes/Logs.tsx` 字段搜索 `<input>` 加 `aria-label="Filter fields"`
- [x] 7.3.3 `src/routes/Logs.tsx` 分页 `«»` / 刷新按钮 5 个加 aria-label
- [x] 7.3.4 `src/routes/Logs.tsx` `<select>` × 2 + `<textarea>` SQL editor 加 aria-label
- [x] 7.3.5 `src/routes/Logs.tsx` 仅含图标的 ChromeButton（RefreshCw / ⋮）加 aria-label
- [x] 7.3.6 `src/shell/chrome.tsx` TimeRangeChip 按钮加动态 `aria-label`（含当前 value）

### 7.4 keyboard-map 键名归一化
- [x] 7.4.1 GLOBAL_KEYMAP 的 `mod+k` 对应 Playwright 的 `Meta+K`（macOS）/`Control+K`（Linux）。规则化层做 `mod+ → Meta+` / `esc → Escape` / `enter → Enter`，否则 chord 测试永远找不到 modifier 键

### 7.5 TimeSeries demo 确定性
- [x] 7.5.1 原 `Math.random()` 噪声使 canvas 像素每次不同，focus-ring 视觉 baseline 第二次跑必 fail。换成 seeded LCG + `Date.now()` 锚到 `2026-05-23T10:00:00Z`，跨运行像素稳定
