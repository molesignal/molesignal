## 1. i18n 装框架

- [x] 1.1 `pnpm -C web add react-i18next i18next`
- [x] 1.2 新建 `web/src/i18n/index.ts`：initReactI18next + 资源 loader（en / zh-CN）
- [x] 1.3 新建 `web/src/i18n/en/`（common / palette / keyboard / nav / errors 5 个 json）+ shell.json
- [x] 1.4 新建 `web/src/i18n/zh-CN/`（同结构，初版翻译）
- [x] 1.5 `App.tsx` mount 时调 `i18n.changeLanguage(useThemeStore.language ?? navigator.language)` — 在 `main.tsx` import + `ThemeBootstrap` 用 useEffect 同步 store ↔ i18next
- [x] 1.6 `<html lang>` 与 `useThemeStore.language` 同步（useEffect 在 ThemeBootstrap）

## 2. UI 文案抽 `t()`

- [x] 2.1 chrome：Topbar / StatusStrip / IconRail / Login 所有可见字符串走 `t()`
- [x] 2.2 palette：CommandPalette + registry 静态 actions 描述走 `t()`
- [x] 2.3 keyboard help：HelpOverlay binding description 改为 `t(...)` key（ShellRoot 的 binding `description` / `category` 全部用 `t('keyboard:bindings.X')` / `t('keyboard:categories.X')`）
- [x] 2.4 errors / toasts：http 拦截器返回 toast 文案走 `t('errors:...')`（sign_in_failed / offline_dev_mode / switch_org_success / switch_org_failure / link_copied）
- [x] 2.5 a11y aria-label / placeholder 走 `t()`（status_strip.command_palette、user_menu、org_switcher、time_picker、settings 等）

## 3. 配色模板

- [x] 3.1 拆 `tokens.css` 出 `tokens-palette-default.css`（保持现 9 色）+ `tokens-palette-high-contrast.css` + `tokens-palette-warm.css`
- [x] 3.2 高对比 palette：增大每对 fg/bg 间 luminance 差，dark/light 双双过 AA + 余量（最低 9.6:1，多数 15:1+）
- [x] 3.3 warm palette：橙系为主，9 色 alias 重新选 hex（所有 pairs ≥ 6:1）
- [x] 3.4 `useThemeStore.palette` 持 localStorage `molesignal-ui-prefs`，`<html data-palette>` 同步（ThemeBootstrap useLayoutEffect）
- [x] 3.5 `pnpm a11y:contrast` 扩展：脚本读 3 套 token 文件分别跑，输出 palette × theme 矩阵，93 个 pair 全绿

## 4. StatusStrip Settings dropdown

- [x] 4.1 `shell/StatusStrip.tsx`：在 avatar 左加 `Settings` 齿轮按钮 trigger（新 `SettingsMenu` 组件）
- [x] 4.2 DropdownMenu 4 个 section：Theme / Palette / Density / Language；每 section 用 RadioGroup 风格 item
- [x] 4.3 当前项 `data-current="true"`，键盘 ↑↓ 可遍历（Radix 内置），Esc 关闭（Radix 内置）
- [x] 4.4 保留旧 sun/moon 单按钮（双入口）

## 5. light 模式黑色残留扫

- [x] 5.1 grep `text-black|bg-black|border-black|color:.\\?black|#000\b` 找到 4 个 hit（sheet.tsx / dialog.tsx / FormDrawer.tsx / StackPortal.tsx 的 `bg-black/N` overlay）
- [x] 5.2 4 hit 全部改为 `bg-overlay` / `bg-overlay-soft` token；tailwind config + tokens.css 加 `--overlay` / `--overlay-soft` 变量
- [x] 5.3 `scripts/check-no-hardcoded-black.ts` 门禁；接入 `pnpm lint`
- [ ] 5.4 light 截图 4 张关键页（home / logs / dashboards / alerts）人工 review 确认无黑色残留（需 playwright 手动截图）

## 6. baseline rebase

- [ ] 6.1 `pnpm playwright test 05-visual.spec.ts --update-snapshots`（40 张，需浏览器）
- [ ] 6.2 `pnpm playwright test a11y-focus-ring.spec.ts --update-snapshots`（16 张，需浏览器）
- [ ] 6.3 PR 描述贴关键 before/after

## 7. 完工校验

- [x] 7.1 `pnpm -C web a11y:contrast` 3 套 palette × 2 theme 全绿（93 pair 0 fail）
- [x] 7.2 `pnpm -C web typecheck` 0
- [x] 7.3 `pnpm -C web lint` 0（含 no-hardcoded-black 规则 0 violation）
- [x] 7.4 `pnpm -C web test:run` 47 用例不退化；`src/keyboard/__tests__/controller.test.tsx` 2 个 pre-existing fail（jsdom 下 `fireEvent.keyDown(window)` 与 controller `document.addEventListener` 不路由，与本 change 无关）
- [ ] 7.5 `pnpm -C web playwright` 全绿（需 playwright + 56 baseline rebase 后）
- [ ] 7.6 `pnpm -C web playwright:perf` 不退化（需手动跑）
- [x] 7.7 `openspec validate web-theming-i18n --type change --strict` 通过
