---
name: review-tenant-isolation
description: Review MoleSignal changes for cross-organization data exposure or mutation across HTTP/gRPC authentication, IAM authorization, services, repositories, PostgreSQL queries, caches, object-store paths, events, and resource shares. Use whenever a change handles org_id, organization_id, system scope, IAM, SQL, caching, storage keys, background processing, or public/cross-org access.
---

# 审查租户隔离

跨组织读取、写入、缓存碰撞或事件串流都按高严重度安全问题处理。

## 当前隔离链路

1. HTTP 认证在 `src/api/http/middleware/auth.rs` 建立 `IamContext`。
2. 权限检查由 `src/api/http/middleware/permission.rs` 和 `src/app/iam/access.rs` 处理。
3. handler 将 `org_id` 或 `organization_id` 传给 service/repository。
4. PostgreSQL 查询、缓存 key、对象存储 path 和异步事件继续携带组织维度。
5. `system_org_id`、platform administrator、公开分享和显式 cross-org grant 是受控例外，不代表可以省略边界检查。

数据库 ID 当前以字符串形式存储，常见类型是 `TEXT` 或 `VARCHAR(64)`；不要错误要求统一改成 PostgreSQL `UUID`。

## 检查项

1. handler 是否从可信 `IamContext` 取组织，而不是信任 body、query、模型输出或外部 webhook 自报的 org。
2. service/repository 方法是否携带明确的 `&Id` 组织参数；全局操作是否有 system/platform 权限证明。
3. `SELECT`、`UPDATE`、`DELETE`、`UPSERT` 和存在性检查是否包含组织谓词。
4. 唯一约束与 `ON CONFLICT` 是否包含正确组织列。
5. 新表是否有适当的 `org_id`/`organization_id`、非空约束和组织前缀索引；真正全局表需有明确理由。
6. cache key、去重 key、rate-limit key、broadcast topic 和 job key 是否包含组织。
7. object store key 是否带组织前缀；公开分享的 snapshot/session token 是否仍绑定 share 与授权策略。
8. worker 扫描多组织数据时，是否逐组织加载配置、license、权限与 repository 查询。
9. SSO/SAML、API token、gRPC cluster token 和 resource share 是否能被替换组织参数绕过。
10. 日志、trace、审计和 intelligence tool context 是否不会把一个组织的数据传到另一个组织。

## 禁止建议

- 不要先全表读取再在应用层按组织过滤。
- 不要用默认组织或 `Option<Id>` 隐式绕过隔离。
- 不要共享缺少组织维度的可变缓存。
- 不要把来自请求体或模型 tool call 的组织 ID 当作授权来源。

## 输出

1. 总体风险结论
2. 缺少组织过滤的查询或写入（file:line）
3. 缓存、对象路径或事件串流风险
4. IAM/权限中间件缺漏
5. 推荐的 WHERE、复合索引或 key 重构
