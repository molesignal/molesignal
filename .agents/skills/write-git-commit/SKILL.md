---
name: write-git-commit
description: Create or review MoleSignal Git commit messages and squash-merge PR titles using the repository's Conventional Commits and git-cliff conventions. Use when the user asks to commit changes, draft a commit message, choose a type or scope, edit a PR title, split commits, or validate commit wording.
---

# 编写 Git 提交信息

先根据用户指定范围和实际 diff 确认改动主题，不要把工作树中的无关修改写进同一条提交说明。

## 格式

```text
<type>(<scope>): <subject>

<body>

<footer>
```

- commit message、PR title、body 和 footer 全部使用英文。
- `scope` 可省略；只使用一个主要 scope。
- `body` 与 `footer` 可省略，但 breaking change 必须写 footer。

## Type

只使用：

- `feat`：新增功能或用户可见行为
- `fix`：修复 bug
- `docs`：仅文档
- `perf`：性能优化
- `refactor`：无行为变化的重构
- `style`：格式、空白或 import 排序
- `test`：仅测试
- `chore(deps)`：依赖更新
- `chore`：维护任务
- `ci`：CI 或自动化
- `revert`：回滚

## Scope

优先使用当前模块或子系统：

- 分层模块：`api`、`app`、`domain`、`infra`、`bootstrap`、`config`、`shared`、`protocol`
- 产品与能力：`intelligence`、`iam`、`alerting`、`ingest`、`query`、`profiling`、`tracing`、`license`、`marketplace`
- 存储与运行时：`parquet`、`tantivy`、`wal`、`cache`、`runtime`
- 其他：`web`、`ui`、`proto`、`ci`、`hooks`、`makefile`

多模块变更选择最主要的 scope；没有明显主模块时省略。

## Subject 与正文

- subject 使用祈使句和小写开头，除专有名词或代码标识符外不大写。
- subject 不加句号，长度不超过 60 个字符。
- subject 写“做了什么”；body 解释“为什么”以及关键权衡。
- body 每行尽量不超过 72 个字符。
- breaking change 使用 `BREAKING CHANGE: <description>`。
- issue 使用 `Refs: #123` 或 `Closes: #123`。
- 不添加 `Co-Authored-By`、AI 署名、生成声明或推广 footer。

## 示例

```text
fix(infra): register resource share migrations

The embedded migrator must include every SQL file because runtime builds do
not scan the migrations directory.
```

```text
feat(intelligence): add tool execution policy controls
```

生成提交信息时，除非用户要求解释，否则只输出最终 message。
