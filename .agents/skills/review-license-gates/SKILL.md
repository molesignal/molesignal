---
name: review-license-gates
description: Review MoleSignal commercial capability gating through LicenseGate, LicenseHolder, CommunityLicense, SignedLicense, handler entry checks, worker checks, and development-only unlocks. Use when changes touch licensed routes, workers, license persistence or verification, intelligence, SSO, federated search, domain management, cloud marketplace, enhanced profiling, or edition behavior.
---

# 审查 License 门禁

当前实现以运行时 License gate 为主。不要套用旧的“商业 crate + 双重 cfg 门禁”假设。

## 当前模型

- `src/shared/license.rs` 定义 `LicenseGate`、`CommunityLicense` 和可热替换的 `LicenseHolder`。
- `src/license/mod.rs` 实现 Ed25519 验签后的 `SignedLicense`。
- `src/bootstrap/license.rs::build_license` 加载持久化 license；失败降级为 `CommunityLicense`。
- `MS_DEV_UNLOCK_FEATURES` 和 `[intelligence].enabled = true` 只能作为明确的本地开发解锁路径。
- `enterprise` 是 edition/build 标记；`ws`、`jemalloc`、`profiling-pprof`、`js-runtime` 是技术性 Cargo feature，不等同于商业授权。
- 顶层产品模块当前通常无条件编译，授权在 HTTP/gRPC 入口或 worker 周期边界检查。

当前代码使用的 License key 包括：

- `sso`
- `federated_search`
- `intelligence`
- `domain_management`
- `cloud_marketplace`
- `profiling_enhanced`

新增或改名 key 时，应定义/复用常量并同步所有入口、测试、配置与授权数据。

## 检查项

1. 每个受限 HTTP/gRPC 入口是否在执行副作用或读取商业数据前调用
   `state.platform.license.has_feature(...)`。
2. 后台 worker 是否在每轮实际工作开始前检查 feature；未授权时应安全跳过且不产生副作用。
3. 未授权 handler 是否返回明确的 `Error::forbidden(...)`，对应 HTTP 403。
4. `CommunityLicense::has_feature` 是否继续恒为 false；不得通过“临时默认开启”绕过。
5. `SignedLicense` 是否验证签名、有效期与持久化版本；日志不得包含原始签名包、token 或私钥。
6. `LicenseHolder` 热替换后，调用方是否始终读取当前 license，而不是缓存旧判断。
7. dev unlock 是否有显式告警，且不会被生产配置无意启用。
8. license 与订阅/计费状态同时参与判断时，是否保持原有 402/403 语义。
9. 新受限能力是否有 community、signed、expired/invalid 和 dev-unlock 测试。

## 输出

1. 总体结论
2. 缺少入口或 worker 检查的位置（file:line）
3. key 不一致或错误码问题
4. community/dev fallback 风险
5. 推荐的最小 patch
