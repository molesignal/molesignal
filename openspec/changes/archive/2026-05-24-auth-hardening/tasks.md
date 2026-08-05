## 1. 重命名 master_key → cipher_key

- [x] 1.1 `mv crates/infra/src/cipher/master_key.rs crates/infra/src/cipher/cipher_root_key.rs`
- [x] 1.2 在新文件里改 struct `MasterKey` → `CipherRootKey`，error `MasterKeyError` → `CipherRootKeyError`，env var `MS_MASTER_KEY` → `MS_CIPHER_KEY`
- [x] 1.3 `crates/infra/src/cipher/mod.rs`：`pub mod master_key` → `pub mod cipher_root_key`；re-export 同步
- [x] 1.4 `crates/infra/src/cipher/cipher_keys.rs`：所有 `MasterKey` import + 类型签名替换
- [x] 1.5 `crates/bootstrap/src/wire.rs`：`MasterKey::from_env()` → `CipherRootKey::from_env()`；变量名 `master_key` → `cipher_root_key`；ZERO_B64 注释更新
- [x] 1.6 grep 验证：`rg 'master_key|MASTER_KEY|MasterKey' crates/ enterprise/ deploy/ conf/ .devcontainer/ docs/ --type-not md` 应零命中（md 文档下一节处理）
- [x] 1.7 `cargo check --workspace` + `cargo test --workspace --lib` 全过

## 2. signing_secrets 表 + repo + 启动期 bootstrap

- [x] 2.1 sqlx 迁移 `20260601000008_signing_secrets_and_api_tokens.sql`：含两张表（本 change 后续 也用）
- [x] 2.2 `signing_secrets` schema：`(id PK, kind, secret BYTEA, is_primary BOOL, created_at_micros, retired_at_micros?)` + partial unique index `(kind) WHERE is_primary`
- [x] 2.3 `crates/infra/src/persistence/repositories/signing_secrets.rs`：CRUD + `get_primary(kind)` + `list_active(kind, retire_window_micros)` + `insert_primary` + `retire(id)`
- [x] 2.4 `bootstrap_or_load_jwt_secret(repo, env_override)`：env > DB > 自生成（**偏离 spec**：因 `crates/infra` 反向 dep `crates/app`，bootstrap 函数与 trait/PgImpl 同 file，放 `crates/infra/src/persistence/repositories/signing_secrets.rs`，不在 `crates/app/src/identity/signing.rs`；调用方 wire/route 已经直依赖 infra，无功能差异）
- [x] 2.5 `IdentityService` 改造：构造期持 `Vec<Vec<u8>>` active_secrets + 知道哪个是 primary；`issue_token` 用 primary；`verify_token` for-loop 多 secret 试
- [x] 2.6 `wire::build_state` 装配 `SigningSecretRepository` + 调 `bootstrap_or_load_jwt_secret`
- [x] 2.7 `crates/api/src/state.rs::AppState` 加 `signing_secrets: Arc<dyn SigningSecretRepository>`
- [x] 2.8 单测：bootstrap_fresh_db / bootstrap_existing_primary / env_override_upserts / multi_secret_verify_rotates / concurrent_bootstrap_race（用 mock repo）

## 3. api_tokens 表 + repo + HTTP CRUD + middleware 双轨

- [x] 3.1 `api_tokens` schema（复用同一 migration）：`(id PK, prefix UNIQUE, secret_hash, org_id, user_id, role, name, expires_at_micros?, last_used_at_micros?, revoked BOOL, created_at_micros)`
- [x] 3.2 `crates/infra/src/persistence/repositories/api_tokens.rs`：CRUD + `find_by_prefix` + `mark_revoked` + `touch_last_used` + `list_by_org`
- [x] 3.3 token 生成辅助 `generate_token_parts` / `assemble_token` / `split_token` / `hash_secret` / `verify_secret`：base62 16+32 + argon2id（**偏离 spec 位置**：放 `crates/infra/.../api_tokens.rs` 同 file 而不是 `crates/app/src/identity/api_token.rs`，原因同上）
- [x] 3.4 `crates/api/src/http/routes/api_tokens.rs`：
  - `POST /api/v1/auth/tokens { name, role?, expires_in_days? }` → 一次性 plaintext
  - `GET /api/v1/auth/tokens` → list（无 secret）
  - `DELETE /api/v1/auth/tokens/{id}` → mark revoked + emit audit
- [x] 3.5 `crates/api/src/http/middleware/auth.rs` 双轨改造：
  - `Authorization: Bearer ms_X` → 拆 prefix+secret → repo.find_by_prefix → argon2 verify → revoked/expired check → AuthContext
  - 否则 → JWT 现有路径
  - `last_used_at` async tokio::spawn 更新
- [x] 3.6 role escalation 校验：token.role 必须 ≤ caller.role（Role allow chain）
- [x] 3.7 单测：generate_token_format / hash_verify_roundtrip / prefix_lookup / revoked_rejected / expired_rejected / role_escalation_blocked

## 4. Rotate / list JWT secrets HTTP

- [x] 4.1 `crates/api/src/http/routes/jwt_secrets.rs`：
  - `POST /api/v1/auth/jwt/rotate`（Owner-only）→ retire current + insert new + reload active set + audit
  - `GET /api/v1/auth/jwt/secrets`（Owner-only）→ list 不含 secret raw
- [x] 4.2 reload 机制：rotate 后内存 active_secrets 立刻刷新；多节点场景靠每 60s 后台 reload 兜底（cluster broadcast 留 follow-up）

## 5. 配置 / settings 调整

- [x] 5.1 `crates/config/src/settings.rs::AuthSettings`：删 `jwt_secret` 字段（保留 `deprecated_jwt_secret` alias 容器以容忍旧 TOML，启动 main 打 deprecation warn；不参与签名）
- [x] 5.2 `conf/config.toml`：删 `[auth] jwt_secret = "..."` 行（仅留 comment 说明锁定场景用 env override）
- [x] 5.3 新增 env var 支持：`MS_AUTH_JWT_SECRET_OVERRIDE`（不放 Settings 字段；wire 直接 `std::env::var`）

## 6. AppState / wire 注入

- [x] 6.1 AppState 加 `signing_secrets` + `api_tokens` 两个 repo
- [x] 6.2 wire.rs 装配，注入 IdentityService 与 middleware
- [x] 6.3 routes/mod.rs 挂 api_tokens + jwt_secrets

## 7. 部署 / devcontainer / 文档同步

- [x] 7.1 `deploy/k8s/20-secret.yaml`：`master_key` 字段 → `cipher_key`；`jwt_secret_override` 字段标 optional（用 MS_AUTH_JWT_SECRET_OVERRIDE 注入）
- [x] 7.2 `deploy/k8s/40-ingester.yaml` / 50-querier / 60-compactor / 70-alert-manager / 90-connector / 30-router：`MS_MASTER_KEY` env → `MS_CIPHER_KEY`；删 `MS_AUTH_JWT_SECRET`（auto-bootstrap），改用 optional `MS_AUTH_JWT_SECRET_OVERRIDE`
- [x] 7.3 `deploy/docker/docker-compose.yaml`：`MS_MASTER_KEY` → `MS_CIPHER_KEY`；删 `MS_AUTH_JWT_SECRET`
- [x] 7.4 `.devcontainer/devcontainer.json`：`MS_MASTER_KEY` → `MS_CIPHER_KEY`；删 `MS_AUTH_JWT_SECRET`（dev 走 DB auto-bootstrap）
- [x] 7.5 `.devcontainer/post-create.sh`：cipher_key 仍由 devcontainer.json env 直接注入全零 base64（dev only，等价语义；named volume 文件挂载路径作为 follow-up——当前对 dev 体验无差异）
- [x] 7.6 `ARCHITECTURE.md`：Part 2 "Audit / Quotas / Cipher keys" 命名更新；Part 2 "Config Watcher"（line 378）immutable 列表去掉 `auth.jwt_secret`；Part 3 新增 `## Auth hardening (auth-hardening change)` 一节
- [x] 7.7 `README.md`：cipher key 段命名更新；新增 JWT auto-bootstrap + rotate 卖点；API token 段已有
- [x] 7.8 `docs/api/openapi.yaml`：原有 `POST/GET/DELETE /auth/tokens` 3 个端点；本 change 新增 `POST /auth/jwt/rotate` + `GET /auth/jwt/secrets` 2 个端点

## 8. 完工校验

- [x] 8.1 `cargo fmt --all` + `cargo clippy --workspace --all-targets`（fmt 干净；clippy 只剩与本 change 无关的既有 warning）
- [x] 8.2 `cargo test --workspace --lib`（104 + 20 + ...全部 passed；`--features enterprise` cargo check 同样通过）
- [x] 8.3 `rg 'master.*key' -i --type rust crates/ enterprise/` 零命中（test fixture 除外）
- [x] 8.4 `openspec validate auth-hardening --strict` → Change 'auth-hardening' is valid
- [ ] 8.5 手动 e2e：dev 启动 → log 看到 "first-run: generated new JWT signing secret" → login 拿 JWT OK → POST /auth/tokens 拿 ms_token OK → 用 ms_token 调 /api/v1/streams OK → POST /auth/jwt/rotate → 老 JWT 还能用（24h grace）→ 新 JWT 用新 secret 签
