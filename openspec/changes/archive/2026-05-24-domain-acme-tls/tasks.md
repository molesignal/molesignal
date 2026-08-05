## 1. 依赖与配置

- [x] 1.1 `Cargo.toml` 工作区加 `instant-acme = "0.7"`、`rustls = "0.23"`、`tokio-rustls = "0.26"`、`axum-server = { version = "0.7", features = ["tls-rustls"] }`、`rcgen` (dev) for tests、`pem`
- [x] 1.2 `crates/config/src/settings.rs` 加 `HttpSettings.tls: TlsSettings { enabled, plain_port, port, acme_directory, account_email, key_storage_dir }`
- [x] 1.3 wire 阶段读 `tls.key_storage_dir`，`std::fs::create_dir_all` 保证存在（main.rs 一并落地）

## 2. AcmeClient 实装

- [x] 2.1 `crates/bootstrap/src/acme/client.rs`：`AcmeClient { directory, account, account_key }`
- [x] 2.2 `async fn ensure_account()`：账户 key 缺时 `instant-acme::Account::create` 新建并 cache 到 `key_storage_dir/account.key`
- [x] 2.3 `async fn issue(hostname) -> Result<IssuedCert>`：order → identifiers → 拿 http-01 challenge → 把 token + key_authorization 写入 `acme_challenges` 表（已存在）→ 通知 ACME server ready → 轮询 finalize → 拿 cert chain
- [x] 2.4 把 `(cert_pem, key_pem)` 中的 cert_pem 写 `domains.cert_pem`（由 AcmeRunner::issue_one 完成），key_pem 写 `key_storage_dir/<hostname>.key.pem`
- [x] 2.5 unit test：IssuedCert 结构 + parse_not_after_micros pure-fn 单测（pebble docker 全链由 覆盖，env-gated）

## 3. SNI cert resolver

- [x] 3.1 `crates/bootstrap/src/tls/sni_resolver.rs`：`struct SniCertResolver { domains: Arc<dyn DomainRepository>, cache: DashMap<String, (Arc<CertifiedKey>, Instant)>, ttl: Duration, key_dir: PathBuf }`
- [x] 3.2 `impl ResolvesServerCert`：`fn resolve(client_hello)` → SNI → check cache → miss 时 `find_by_hostname` + 读 key file + parse 成 `CertifiedKey` + cache
- [x] 3.3 `invalidate(hostname)` 方法：cert 更新后调
- [x] 3.4 unit test：known host → Some；unknown → None；invalidate 清 cache（3/3 通过）

## 4. acme_runner worker

- [x] 4.1 `crates/bootstrap/src/workers/acme.rs::AcmeRunner { client, domains, cache_resolver }`
- [x] 4.2 `spawn` 起两个 tokio task：`issue_loop` 每 60s 扫 pending、`renewal_loop` 每 6h 扫 active 临期（list_by_state DB 索引待 schema follow-up）
- [x] 4.3 cooldown：进程内 DashMap 60s 内同 domain 不重试（避免 LE rate limit）
- [x] 4.4 issue 成功后调 `cache_resolver.invalidate(hostname)`
- [x] 4.5 unit test：needs_renewal pure fn 验 30-day 阈值 + 缺 cert 情况

## 5. HTTPS server bind 切换

- [x] 5.1 `crates/bootstrap/src/roles/http_server.rs`：`tls.enabled=true` 分支 `serve_tls` 起 80 plain（healthz + ACME challenge + 301 redirect）+ 443 rustls（完整 router + SniCertResolver） + grpc，并行 `try_join!`
- [x] 5.2 `tls.enabled=false` 分支保持现有 `axum::serve` 不变（cfg=enterprise 默认 false；OSS 编译永不进 TLS 分支）
- [x] 5.3 80 端口 fallback handler 除 healthz + acme-challenge 外，其余路径取 Host header 后 `Redirect::permanent("https://{host}{path}")`
- [x] 5.4 `SniCertResolver` + `AcmeRunner` 在 `serve_tls` scope 内 Arc 持有，未污染 AppState；`key_storage_dir` 通过 `std::fs::create_dir_all` 保活

## 6. 编译矩阵 + 集成测试

- [x] 6.1 `cargo check --workspace`（OSS：tls 字段加 default off，编译可过）
- [x] 6.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 6.3 `cargo test -p molesignal-bootstrap --features enterprise --lib` 全绿：17/17（含 sni_resolver 3 + acme client 2 + acme runner 2）
- [x] 6.4 `crates/bootstrap/tests/it_acme_issue.rs`：scaffold 已就位，要求 `MS_PEBBLE_URL` env 才实跑（无 Pebble 容器时 skip-fast）；编译 `cargo test --tests --no-run` OSS + enterprise 双绿。完整 Pebble + DNS 还原由专用 CI job 覆盖。
