# 给 MoleSignal 的贡献指南

> English version: [CONTRIBUTING.md](CONTRIBUTING.md)

感谢考虑给项目贡献！本文涵盖开发环境搭建、代码约定与 PR 流程。

报告漏洞请遵循 [SECURITY.md](SECURITY.md)，**不要** 在公开 issue 披露。

参与即视为接受我们的 [行为准则](CODE_OF_CONDUCT.md)。

## 目录

- [从哪开始](#从哪开始)
- [开发者来源证书（DCO）](#开发者来源证书dco)
- [开发环境](#开发环境)
- [代码约定](#代码约定)
- [测试](#测试)
- [Commit 信息](#commit-信息)
- [分支与发布通道](#分支与发布通道)
- [Pull request 流程](#pull-request-流程)
- [需要帮助](#需要帮助)

## 从哪开始

- 新贡献者：先扫一遍 [README](README.md)，然后过一下 [ARCHITECTURE.md](ARCHITECTURE.md) —— DDD 分层与跨信号关联是大多数改动的承重墙。
- 在做的设计在 [`openspec/changes/`](openspec/changes) 下，每个 change 都有 `tasks.md`，你可以挑一项还没勾选的任务认领。
- README 的 "Status" 表展示我们最希望得到帮助的方向（生产硬化、demo 数据集、跨信号关联的真实用例）。
- 有 `good first issue` 标签时优先选它；没有的话，typo / 文档修复永远欢迎。

## 开发者来源证书（DCO）

我们使用 [Developer Certificate of Origin](https://developercertificate.org/)（DCO）保证来源链清晰。**每个 commit 必须带 `Signed-off-by:` trailer**，声明这段代码是你写的（或者你有权按本项目 License 提交）。

提交时自动添加：

```bash
git commit -s -m "your message"
```

漏写就 `git commit --amend -s`；多个 commit 用 `git rebase --signoff <base>` 批量补上。

我们不要求 CLA —— DCO trailer + 每个源文件顶部的 Apache-2.0 SPDX 头就够。

## 开发环境

依赖：

- Rust toolchain 已经被 [`rust-toolchain.toml`](rust-toolchain.toml) 钉住，`rustup` 会自动用对。
- 一个 nightly rustfmt（因为 `imports_granularity` / `group_imports` 是 nightly 选项）：
  `rustup toolchain install nightly --profile minimal --component rustfmt`
- `protoc`（例如 `apt-get install protobuf-compiler` 或 `brew install protobuf`）以及 [Buf CLI](https://buf.build/docs/installation)。
- Docker（跑集成测试和 sandbox compose 用）。
- 改 `web/` 还需要 Node 20 + `pnpm` 9。

最小快路径：

```bash
make proto                                          # 生成 gRPC 代码
cargo +nightly fmt --all                            # 跟 rustfmt 配置对齐
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib --bins                 # 快：仅单元 + bin 测试

# Sandbox：Postgres + MinIO + molesignal standalone
docker compose -f deploy/docker/docker-compose.yaml --profile standalone up
```

Web：

```bash
pnpm -C web install --frozen-lockfile
pnpm -C web typecheck && pnpm -C web lint
pnpm -C web test:run
pnpm -C web dev          # vite dev server
```

通过 `make install-hooks` 安装的 pre-commit 钩子会本地校验 license header、nightly fmt 和 clippy —— 请不要用 `--no-verify` 绕过。

## 代码约定

- **DDD 分层**：依赖箭头必须从外往内：`bootstrap → api → app → domain → shared`；infra (`crates/infra`) 实现 `domain` 的端口，不能反向。
- **不过度抽象**：三处雷同代码比给两处用一次的通用 helper 好。
- **注释只解释 *为什么***：命名负责"是什么"，注释留给不变量、变通方案、出乎意料的约束。
- **不留向后兼容垫层**：除非明确需要；删掉的代码就让它删干净。
- **public 类型一句话说明它为什么存在**；内部 helper 不需要。
- **License 头**：每个 Rust / TS 源文件都从 `licensure` 钩子要求的 SPDX banner 开始。

## 测试

- 单元测试紧贴被测代码（`#[cfg(test)] mod tests`）。
- 集成测试在 `crates/*/tests/it_*.rs`。
- 需要 Docker（Postgres testcontainer / MinIO / Pebble 等）的，必须放到 `MS_RUN_IT=1` 后面：

  ```rust
  if common::skip_unless_enabled() { return; }
  ```

  整套集成测试跑法：

  ```bash
  MS_RUN_IT=1 cargo test -p molesignal-bootstrap --tests -- --test-threads=1
  ```

- 改 UI / 前端的工作，请在浏览器里走通再算"完成" —— 类型检查和单元测试抓不住交互回归。
- 改查询计划或多租户代码，`crates/bootstrap/tests/it_multitenant.rs` 与 `it_planner_rewrite.rs` 是必须保持绿的契约测试。

## Commit 信息

- 标题 ≤ 72 字符，祈使语气（`fix(api): …`、`feat(query): …`、`refactor(infra): …`）。
- 优先用 Conventional Commits 前缀（`feat` / `fix` / `refactor` / `docs` / `test` / `ci` / `chore`）。
- 正文聚焦 *为什么*，*做了什么* 看 diff。
- 标题、正文、trailer 都写英文。（issue / PR 评论的对话可以中文或英文，由你与维护者决定。）
- 每个 commit 必须带 `Signed-off-by:`（见 [DCO](#开发者来源证书dco)）。

## 分支与发布通道

按分支区分四个发布通道。所有通道逐级晋升同一个 Cargo `release` 制品，部署时通过运行时 `RELEASE_CHANNEL` 标记成熟度：

| 分支    | 通道   | tag 命名   | 来源 |
|---------|--------|-----------|------|
| `alpha` | alpha  | `vX.Y.Z`  | feature PR |
| `beta`  | beta   | `vX.Y.Z`  | 从 `alpha` promote |
| `rc`    | rc     | `vX.Y.Z`  | 从 `beta` promote |
| `main`  | stable | `vX.Y.Z`  | 从 `rc` promote |

跨通道用 *同样* 的 semver tag；workflow 根据 tag commit 在哪条分支上推断通道（`main > rc > beta > alpha`，更稳定的优先）。详见 [`.github/workflows/release.yml`](.github/workflows/release.yml)。

制品由 Git SHA 和 CI `BUILD_ID` 共同标识。晋升时必须复用该 build ID 对应的不可变二进制或镜像，不得重新编译。

日常 PR 默认目标分支是 `alpha`（如果 `alpha` 还没建出来，先指 `main`，等通道流程建好再调整）。

## Pull request 流程

1. 非微小变更先开 issue / discussion 对齐方向 —— 不希望你周末写完一个大重构才发现方向有分歧。
2. 从目标通道 fork 分支，PR 保持小而聚焦，一个 PR 只做一件逻辑事。
3. 行为面有变更时同步更新相关文档（`README.md`、`ARCHITECTURE.md`、crate 内 doc comment）。
4. push 前确保 CI 必跑项本地全绿：
   - `cargo +nightly fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --lib --bins`
   - 涉及 HTTP / wire / 持久化的特性，跑 `crates/bootstrap/tests/` 下对应的 `it_*.rs`，记得带 `MS_RUN_IT=1`。
5. push、开 PR、填模板。请在 PR 描述里写清：
   - 动机（解决什么问题；改了哪些用户可见行为）。
   - 测试计划（你本地跑了什么；什么还没测，原因是什么）。
   - UI / API 变更附截图或 curl 示例。
6. 评审反馈用追加 commit 来响应（merge 时我们会 squash —— 但在那之前历史得是可读的）。

## 需要帮助

- 架构 / 设计问题：开 GitHub Discussion，或者在对应 `openspec/changes/` 文档下评论。
- 非安全相关 bug：普通 GitHub issue。
- 安全相关：[SECURITY.md](SECURITY.md)。
- 行为规范相关：[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

暂时还没有 Discord / Slack —— 等贡献者规模大到 GitHub 异步沟通跟不上节奏时再建。
