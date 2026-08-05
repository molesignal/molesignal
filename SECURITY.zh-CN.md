# 安全政策

> English version: [SECURITY.md](SECURITY.md)

感谢你关注 MoleSignal 的安全状况。项目当前处于 1.0 前阶段，自托管部署；它涉及不少安全敏感特性（多租户查询改写、cipher key 信封加密、JWT 签名密钥轮换、审计日志、ingest 配额等），所有安全报告我们都会严肃对待。

## 支持的版本

1.0 之前，只有 `main` 分支会获得安全修复，老 tag 的回填属于尽力而为。

| 版本                             | 是否维护 |
|----------------------------------|----------|
| `main`（HEAD）                   | ✅       |
| `main` 上最新发布的 tag          | ✅       |
| `beta` / `alpha` 通道            | ✅（仅前向修复）|
| 更老的 tag                       | ❌       |

1.0 发布后我们会重新审视该表并补充 LTS 政策。

## 漏洞上报

**请不要在公开 issue 里披露漏洞**，请从下面两个私密渠道里任选其一：

- **GitHub 私密漏洞上报**：<https://github.com/molesignal/molesignal/security/advisories/new>
- **邮件**：<security@molesignal.io>（需要 PGP 时请索要公钥）

请在报告里包含：

- 问题简述与你观察到的影响。
- 最小复现步骤（配置片段、请求体、命令序列）。
- 你测试时的 commit SHA 或 release tag。
- 你建议的严重等级和（如果有）缓解方案。

## 范围

属于范围内的：

- MoleSignal 服务端（`crates/bootstrap`、`crates/api`、`crates/app`、`crates/infra`、`crates/domain`、`crates/shared`）
- `web/` 下的前端
- `deploy/` 下的官方 Docker 镜像与 Compose / Kubernetes manifest
- 跨租户数据泄露、认证 / 授权绕过、签名密钥 / cipher key 泄露、ingest 注入，以及任何与"文档承诺的多租户隔离保证"不一致的行为

不属于范围（请不要作为漏洞上报）：

- 必须依赖极端非默认配置才能触发的 DoS（比如把 `[wal]` flush 调到完全无背压再灌流量）
- 在 MoleSignal 代码路径上不可达的上游依赖问题——请直接上报上游
- 仅出现在你自己 patch 过的非默认 build 上的问题
- 已经在公开 roadmap 中跟踪的"缺失的安全加固"

## 你能期待的响应节奏

- **3 个工作日内确认** 收到。
- **7 个工作日内完成初判**（确认 / 非漏洞 / 需更多信息）。
- 高危 / 严重问题 **30 天内** 给出修复或缓解方案，低危会更长，通过 advisory 沟通。
- 我们协同披露：修复合入 `main` 并发版后，会公布 advisory 并在 changelog 致谢，除非你要求匿名。

我们是小团队，如果超出上面的时间窗一点，请耐心；如果完全收不到回复，欢迎在 advisory 里直接 ping。

## 给运维者的加固提醒

如果你把 MoleSignal 跑生产，以下这些不是可选项：

- **设置 `MS_CIPHER_KEY`** 为真实的 32 字节 base64 密钥（全零兜底仅 dev 用且会 WARN 日志）。
- 首次启动后 **轮换 JWT bootstrap 签名密钥**（`POST /api/v1/auth/jwt/rotate`）。
- 在 ingress 层 **限制 `/api/v1/_*` 与 `/metrics`** 只对内部调用方开放——它们是管理面。
- 改 query 代码时，**在你的 fork 跑 planner-rewrite 测试**；`it_multitenant.rs` 是租户隔离的契约测试。
- 共享 ingest 时务必 **开启 per-org 配额**，让失控的 producer 在 413/429 上自停，而不是把邻居拖死。

加固部署时遇到的问题，恰恰是我们最想收到的报告。

## 致谢

按本流程报告并希望署名的研究者会在对应 release 的 advisory 与 `CHANGELOG.md` 中被致谢。
