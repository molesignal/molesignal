## Context

3 个并存问题（详见 proposal.md），共同点是"启动期密钥管理 + middleware 改造"。一次性纳入比拆 3 个 change 更经济（共用 migration + 共用 wire 改造点）。

参考：
- OpenObserve 实际是 OSS 不用 JWT，走 HTTP Basic Auth + per-user token 列；企业版 JWT 走 IdP JWKS（不自管 secret）
- 用户明确要 JWT（OSS + 企业版），所以不照搬 OO，而是用"DB 持久化 signing_secrets"的工业标准方案（Django SECRET_KEY / Rails secret_key_base / Phoenix endpoint secret_key_base 同模式）

当前状态（已 grep 确认）：
- `crates/infra/src/cipher/master_key.rs` 27 处引用 `MasterKey` / `MasterKeyError`
- `crates/app/src/identity/mod.rs::IdentityService` 持 `auth: AuthSettings`，`issue_token` / `verify_token` 用 `self.auth.jwt_secret`
- `crates/config/src/settings.rs::AuthSettings.jwt_secret: String`
- `crates/bootstrap/src/wire.rs` 装配 `IdentityService::new(users, orgs, memberships, settings.auth.clone())`
- `crates/api/src/http/middleware/auth.rs` 仅识别 `Bearer <JWT>`
- `api_tokens` 表 / repo / route 全无（spec 没落代码）
- `signing_secrets` 表 / repo / 启动期 bootstrap 全无

## Goals / Non-Goals

**Goals:**
- 改名一致：env / struct / 模块文件 / 文档 / k8s manifest / docker-compose / devcontainer 全统一
- JWT secret **永不在配置文件或镜像里**：首启动 DB 自生成；运维**默认不需要任何 ENV 注入**
- Rotate JWT secret 走在线 API，**不需要重启**，且老 token 在 24h grace 内仍可验证
- API token 体系跟 OO 拉齐：`ms_<prefix>_<secret>` + argon2 存储 + middleware 双轨
- 整个 change 单 PR 合并；OSS 主仓 `cargo build --workspace` 持续编译通过

**Non-Goals:**
- 不引入 OAuth2 / refresh token（JWT 过期重新登录；API token 长期有效）
- 不引入 cookie-based session（仍 stateless bearer）
- 不动 cipher-keys 的 AES-256-GCM 算法本身，也不动 envelope 模型
- 不为旧 `MS_MASTER_KEY` 保留 backward-compat 别名（首次发布前砍干净）
- 不实施 KMS（AWS / GCP / Vault）外部托管 root key（独立 follow-up change；本 change 只解决命名 + JWT + API token）

## Decisions

### D1：struct 命名 `CipherRootKey` 不是 `CipherKey`

`CipherKey` 名已被 `crates/infra/src/cipher/cipher_keys.rs` 的 user-level key 占用（user 通过 HTTP 创建的命名 key，是 DEK 语义）。重名会逼读代码人去消歧义。

`CipherRootKey` 同时编码了"root"的语义：它是包 user-level cipher key 的 envelope KEK。OO 称之为 "MasterKey" 是历史遗留命名；行业标准是 KEK + DEK；本项目折中走 `CipherRootKey` + `CipherKey`（root 包 user）。

**备选 1**：把现有 `CipherKey` 改名 `DataCipherKey`（标准 DEK 名），然后把 `MasterKey` 改名 `CipherKey`。**否决**：现有 cipher_keys 表的 HTTP API 路径 `/api/v1/cipher_keys` 已经发布过，改名表面 → 改 API → 破坏外部契约。

**备选 2**：env var 也用 `MS_CIPHER_ROOT_KEY` 与 struct 对齐。**否决**：用户明确 `MS_CIPHER_KEY`；env var 名比 struct 名更面向运维，运维不需要知道 envelope 概念。

### D2：JWT secret 走 DB 持久化 + 多 secret 并存，而不是单 secret 文件

OO 走的"Basic Auth + per-user token 列"虽然简单但有限：
- 不支持短期 session token（每次请求都传长期 token，泄露面更大）
- 不支持 JWT claims（org_id / role 这些每次得查 DB）

我们要 JWT（用户明确），所以选行业标准 `signing_secrets` 表 + 启动期 bootstrap：
- env 显式注入路径仍保留（CI deterministic / multi-region 共享秘钥场景）
- 缺 env → 读 DB primary
- 都没 → `OsRng::try_fill_bytes` 32B 生成 + INSERT INTO + log info

**verify_token 必须支持多 secret**（兼容 rotate window）：
```rust
for s in &self.active_secrets {  // primary + 24h 内 retired 的
    if let Ok(ctx) = jwt::decode(token, s) { return Ok(ctx); }
}
return Err(unauthorized);
```

**rotate 算法**：
```sql
UPDATE signing_secrets SET is_primary = FALSE, retired_at_micros = NOW
  WHERE kind = 'jwt' AND is_primary = TRUE;
INSERT INTO signing_secrets (id, kind, secret, is_primary, created_at_micros)
  VALUES (?, 'jwt', ?, TRUE, NOW);
```
24h 后老 secret 被 `list_active` 排除（自然 reap）。后台 cleanup job 每天扫 retired 超 48h 的 `DELETE`。

### D3：双 token middleware 用前缀分发，不用 try-decode-fallback

```rust
let bearer = ...;  // Bearer <X>
if bearer.starts_with("ms_") {
    // API token 路径：split prefix + secret → argon2 verify → AuthContext
} else {
    // JWT 路径：multi-secret verify_token → AuthContext
}
```

前缀分发优势：
- 路径选择 O(1)，不需要 try-decode JWT 失败后才 fallback
- 错误日志清晰（"invalid api token" vs "invalid jwt"）
- 前缀本身充当 namespace，未来加 `gho_` / 其它格式不会冲突

### D4：API token 格式 `ms_<16-prefix>_<32-secret>` 与 GitHub 一致

- `ms_`：scheme namespace（同 `ghp_` `gho_` `xoxb-` 风格）
- prefix 16 字符 base62：DB 索引列；用户 `GET /tokens` list 时显示前 8 + `...` 做识别
- secret 32 字符 base62：仅创建时返一次；DB 只存 argon2 hash
- 完整 token ≈ `ms_aB3kZ1xT9pQrU7nM_dFgHjKl8eRvNcWxYz4tBmEqPaS2vG6QzD7uHcXp`

prefix 唯一约束 → O(1) 查找（不需要全表扫 argon2 比对）。argon2 仅在拿到候选行后跑一次。

### D5：删 `[auth].jwt_secret` 字段是 breaking 但选了

替代方案是保留字段作 "override hint"，缺值时 fallback DB bootstrap。

**否决**：保留只制造混淆—— "我配了 jwt_secret 为什么改了不生效？"答："因为只在 DB 空时才用"。这种 surprising 行为是反 Postel's Law。

干净做法：字段删除，文档说明：
- 默认（99% 部署）：什么都不配，自动生成
- 锁定场景：设 `MS_AUTH_JWT_SECRET_OVERRIDE` env var，启动时强制写入 DB primary（INSERT ... ON CONFLICT 更新）

升级路径文档（首次升级时跑一次 SQL）：
```sql
INSERT INTO signing_secrets (id, kind, secret, is_primary, created_at_micros)
  VALUES (gen_random_uuid(), 'jwt', <old_jwt_secret_bytes>, TRUE, ...);
```

### D6：cleanup job 不进本 change

- retired_at > 48h 的 signing_secret 删除
- revoked = true 的 api_token 删除（90d window 后）
- expired api_token 自动失效（middleware 已校验；不主动删表行）

这些是 GC 性质，与本 change 主路径解耦。**作为 follow-up issue 立单**，不阻塞本 change merge。

## Risks / Trade-offs

- **[Risk] 改名遗漏一处导致 prod 起不来**：grep `master_key|MASTER_KEY|MasterKey` 兜底；CI 加一条 `cargo build` 不带 `MS_MASTER_KEY` env，必须能起。**Mitigation**：tasks.md 列每处具体改动；提交后跑 `rg 'master.*key' -i` 全项目验证零命中（文档示例除外，需用 alllowlist）。
- **[Risk] 多 secret verify 性能退化**：每个 JWT verify 试 N 个 secret，rotate 当下 N=2，最坏 4-5（频繁 rotate）。**Mitigation**：active secrets 缓存在 IdentityService 启动期 + rotate API 触发的 reload；不每次请求查 DB。
- **[Risk] 启动期 race：两个 ingester 同时启动都首次 bootstrap**：会 INSERT 两行 primary（应仅一行 is_primary=TRUE）。**Mitigation**：partial unique index `CREATE UNIQUE INDEX uq_signing_primary ON signing_secrets(kind) WHERE is_primary`；冲突方 retry-read。
- **[Risk] argon2 verify 对 API token 慢（~10-50ms）**：高 QPS 场景每请求都跑会拖累。**Mitigation**：内存 LRU cache `(prefix → AuthContext)` 5min TTL；revoke 时显式 invalidate。
- **[Risk] API token 一旦泄露 = 长期凭据无 refresh**：与 JWT 短期不同，API token 默认不过期。**Mitigation**：用户必须显式设 `expires_in_days`（rec ≤ 365）；revoke endpoint 始终可用；rotate workflow 文档化。
- **[Risk] BREAKING `[auth].jwt_secret` 字段删除**：现有 `conf/config.toml` 含这个字段会拒绝解析（serde 严格模式）。**Mitigation**：confirm 项目尚未上 prod；如果上了，加 `#[serde(default, alias = "jwt_secret")]` 兜底。当前 status 是 pre-1.0，可以激进。
- **[Risk] 改名打破已部署 dev 容器的 cipher_keys 解密**：旧 dev 用 `MS_MASTER_KEY` 全零；新版用 `MS_CIPHER_KEY` 也是全零。**值相同 = 解密可行**。dev 容器 wipe 一次也无碍（dev 数据）。

## Migration Plan

无生产迁移（pre-1.0）。开发者升级步骤：

1. `git pull`
2. **如果有自定义 `MS_MASTER_KEY` env**：改名为 `MS_CIPHER_KEY`，值保持不变（同 32B base64）
3. **如果 `conf/config.toml` 含 `[auth] jwt_secret = "..."`**：删该行（自动 DB 生成接管）
4. `cargo build --workspace` —— 应直接通过
5. 首次启动会在 log emit `first-run: generated new JWT signing secret, kid=...`
6. 业务 API 全部正常工作；现有 JWT token 不再有效（重新登录）；新 token 可继续 7d
7. （可选）建第一个 API token：`POST /api/v1/auth/tokens { name: "ci-deploy", expires_in_days: 365 }` → 拿一次性 plaintext 注入 CI

回滚：删 `signing_secrets` 表 + 恢复 `[auth].jwt_secret` 配置（数据无损）。

## Open Questions

1. **JWT secret 的 retire window 是否需要可配？** 默认 24h 还是 1h / 7d？倾向 24h（用户登录会话 SLA），但 GitHub OAuth refresh 是 8h 量级。后续可配。
2. **API token 是否要支持 fine-grained scope？** 现 spec 只有 role（与 user 同），不能"只读这个 stream 不写其它"。FGA capability 已落（企业版），后续可让 token 携带 scope 引用 FGA policy。本 change 不做。
3. **是否给 OSS 也加 service account（无 user 绑定的 token）？** 当前 `api_tokens.user_id` NOT NULL。OO 有 service_accounts 概念。倾向：本 change 不做；service account 是独立 capability。
