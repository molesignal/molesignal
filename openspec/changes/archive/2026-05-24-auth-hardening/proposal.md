## Why

观测设计走查发现 3 个鉴权/加密层面问题需要在 1.0 之前堵掉：

1. **`master_key` 命名不当 + 单层语义** —— 当前 `MasterKey` struct + `MS_MASTER_KEY` 环境变量带 master/slave 时代命名，并且 cipher 模块的两层 envelope（root-key 包 user-key）没在命名上体现。OpenObserve 同义概念叫 "encryption key"。
2. **JWT secret 单点持久化缺失** —— `IdentityService` 从 `[auth].jwt_secret` 单一字符串读签名 secret，没有"首启动自动生成 + DB 持久化 + 重启复用"；运维必须手动产生 + k8s Secret 注入；rotate 唯一办法是重启所有节点（所有 token 立刻失效）。devcontainer / docker-compose 里都是 `"dev-jwt-secret-replace-in-prod"` 这种字面值——明显反模式。
3. **API token 体系仅在 spec，没代码** —— `production-core-engine` 的 `api_tokens` 表 + `ms_*` 长期 token 在 spec 里完整描述，但代码层只有 file_download_tokens 这个无关 token 表，middleware 不支持 `ms_` 前缀，CI / agent / SDK 没法用长期 token。

本 change 把 3 个问题一次性解决。Backward-compat 不必（项目尚未真上 prod）。

## What Changes

### 1. 重命名 `master_key` → `cipher_key`（含命名冲突处理）

- **env var**：`MS_MASTER_KEY` → `MS_CIPHER_KEY`
- **struct**：`MasterKey` → `CipherRootKey`（**不**直接叫 `CipherKey` 因为已经有同名 user-level struct；新名同时体现 envelope 双层语义：root-key 包 user-key）
- **file**：`crates/infra/src/cipher/master_key.rs` → `cipher_root_key.rs`
- **error**：`MasterKeyError` → `CipherRootKeyError`
- **wire 调用**：所有 `MasterKey::from_env()` 等点位同步
- **文档全替换**：spec / ARCHITECTURE.md Part 2 "Audit / Quotas / Cipher keys" / README 运维指南 / .devcontainer/devcontainer.json env 注入 / deploy/k8s/20-secret.yaml + 40-ingester.yaml 等所有 role manifest / deploy/docker/docker-compose.yaml

**BREAKING**: 旧的 `MS_MASTER_KEY` env var 不再识别（无 fallback；首次发布前砍干净）。

### 2. JWT signing secrets：首启动生成 + 持久化 + rotation

- 新 sqlx migration `signing_secrets` 表：`{ id, kind ('jwt'|'cookie'|...), secret BYTEA, is_primary BOOL, created_at_micros, retired_at_micros? }`
- 新 `crates/infra/src/persistence/repositories/signing_secrets.rs`：CRUD + `get_primary(kind)` + `list_active(kind)`（含 retire window）+ `insert_primary` + `retire`
- 新 `crates/app/src/identity/signing.rs::bootstrap_or_load_jwt_secret`：env override → DB primary → 首启动 `rand::OsRng` 32B 生成 + 持久化 + emit info 日志
- `IdentityService` 改造：构造时持 `Vec<Vec<u8>>` active secrets（primary + 24h 内 retired），`issue_token` 用 primary，`verify_token` 多 secret 试验
- `[auth].jwt_secret` 字段从 settings 删（**BREAKING**，外部覆盖路径改用新 env var `MS_AUTH_JWT_SECRET_OVERRIDE`，留给 CI 显式锁定场景）
- 新 HTTP endpoint：
  - `POST /api/v1/auth/jwt/rotate`（Owner-only）—— 生成新 primary + 老的 retire，retire_at = now + 24h
  - `GET /api/v1/auth/jwt/secrets`（Owner-only）—— list 不返 secret raw，只返 `{id, created_at, retired_at?, is_primary}`
- **OSS + 企业版都用 JWT**（用户明确要求；现有 `/api/v1/auth/login` 路径不变，只是 secret 来源换了）

### 3. API tokens 落地（OO 模式）

- 新 sqlx migration `api_tokens` 表：`{ id, prefix VARCHAR(16) UNIQUE, secret_hash, org_id, user_id, role, name, expires_at_micros?, last_used_at_micros?, revoked BOOL, created_at_micros }`
- 新 `crates/infra/src/persistence/repositories/api_tokens.rs`：CRUD + `find_by_prefix` + `mark_revoked` + `touch_last_used`（best-effort）
- 新 `crates/api/src/http/routes/api_tokens.rs`：
  - `POST /api/v1/auth/tokens { name, role, expires_in_days? }` → 一次性返 plaintext `ms_<16-char-prefix>_<32-char-secret>`；DB 只存 argon2(secret)
  - `GET /api/v1/auth/tokens` → list（不返 secret，只返 prefix + role + expires_at + last_used_at + revoked + name）
  - `DELETE /api/v1/auth/tokens/{id}` → mark revoked
- `crates/api/src/http/middleware/auth.rs` 双轨改造：
  - Bearer token 前缀 `ms_` → 拆 prefix + secret → 查 `api_tokens` 表 by prefix → argon2 verify secret → revoked/expired 返 401 → 注入 `AuthContext { user_id, org_id, role }`
  - 否则按 JWT 走现有 `IdentityService::verify_token`
  - `last_used_at` 异步 `tokio::spawn` 更新（best-effort，不阻塞响应）
- audit middleware（已有）自动覆盖 token issue/revoke（spec 已要求）

### 4. 文档 / 部署同步

- ARCHITECTURE.md Part 2 "Audit / Quotas / Cipher keys" 命名更新 + Part 3 加 `auth-hardening` 一节
- README 运维指南 master key → cipher key
- .devcontainer/devcontainer.json：删 `MS_AUTH_JWT_SECRET`（dev 走 DB auto-bootstrap）；`MS_MASTER_KEY` → `MS_CIPHER_KEY`
- .devcontainer/post-create.sh：仍然在 named volume 里幂等生成 cipher key 32B 文件
- deploy/k8s/20-secret.yaml：`master_key` → `cipher_key`；`jwt_secret` 字段保留为 optional override（用 `MS_AUTH_JWT_SECRET_OVERRIDE` 注入）
- deploy/k8s/40-ingester.yaml + 50/60/70/90：env var rename
- deploy/docker/docker-compose.yaml：env var rename
- docs/api/openapi.yaml：加 3 个新 endpoint（POST/GET/DELETE auth/tokens + POST/GET auth/jwt）

## Capabilities

### New Capabilities

- `signing-secrets`：JWT signing key 启动期 bootstrap + DB 持久化 + 多 secret verify 兼容 rotation window + Owner-only rotate / list HTTP endpoints
- `api-tokens`：long-lived bearer token `ms_<prefix>_<secret>` + argon2 存储 + middleware 双 token 路径（JWT 或 ms_）

### Modified Capabilities

- `cipher-keys`：env var + struct + 模块名重命名（核心算法 + envelope 模型不变）
- `identity`：JWT secret 来源从配置文件单值变 DB 持久化多 secret；移除 `[auth].jwt_secret` 字段（**BREAKING**）；middleware 增加 `ms_*` token 识别路径
- `audit`：jwt rotate + api token issue/revoke 自动落 audit_events

## Impact

- **代码**：crates/infra (+2 repo) / crates/app (+1 service) / crates/api (+1 route, middleware 改造) / crates/config（删字段）/ crates/bootstrap（wire 改）/ enterprise/（无）
- **API**：5 个新 HTTP endpoint，1 个 middleware 双轨改造
- **数据库**：2 张新表（signing_secrets / api_tokens）+ 1 个删字段（auth.jwt_secret，仅 config）；启动期 auto-bootstrap 一行 signing_secrets
- **依赖**：无新增（argon2 / rand / jsonwebtoken 都已在 workspace）
- **运维**：
  - `MS_MASTER_KEY` env var 用户必须改名为 `MS_CIPHER_KEY`（首次升级唯一手动操作）
  - `[auth].jwt_secret` 配置项不再生效（DB 自动管，运维通常**不需要**任何 ENV 注入）
  - 想锁定特定 JWT secret（CI / multi-region 同步场景）：用 `MS_AUTH_JWT_SECRET_OVERRIDE` env
- **测试**：约 8-12 套新单测 + 3 套集成测试（it_jwt_bootstrap / it_jwt_rotate / it_api_token_lifecycle）
- **Non-goals**：不实施 OIDC / SSO 改造（spec 已落）；不动 cipher-keys 算法本身；不引入 cookie-based session（仍是 stateless bearer）；不做 OAuth2 token introspection / refresh token（API token 不过期 = 不需要 refresh；JWT 过期重新登录）
