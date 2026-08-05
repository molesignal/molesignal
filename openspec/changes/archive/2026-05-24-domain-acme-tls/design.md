## Context

`domains` 表 + `acme_challenges` 表 + CRUD HTTP route + 顶层 `/.well-known/acme-challenge/{token}` 已就位（spec M1）。enterprise crate `domain-management` 提供了 `hostname_valid` / `needs_renewal` / `renewal_cutoff_micros` 等 pure helper，但 ACME client 是 trait stub，没有真实 impl，也没有 TLS server。

本 change 把这条链接通。

## Goals / Non-Goals

**Goals:**
- 真实 LetsEncrypt（或 Pebble for test）发证流程跑通
- 自动 30 天前续期
- HTTPS server 用 SNI 选证书，无重启上线新域名

**Non-Goals:**
- 不实装 DNS-01 challenge（先 http-01 因为不依赖 DNS provider 集成；wildcard 证书留 follow-up）
- 不实装 multi-region cert sync（多 router 实例之间共享证书走 DB；不引 Vault / 自研 sync 协议）
- 不实装 cert revocation 触发逻辑（用户删 domain 时不主动撤销，让 cert 自然过期）
- 不实装 EAB external account binding（多数 hosted CA 用，自部署 LE 不需要）

## Decisions

### D1：用 `instant-acme` 而不是 `acme-lib` / `let-rs`

`instant-acme` 是 rustls-stack pure async，与 axum / tokio 一致；其他要么阻塞 IO 要么基于 openssl-sys。

### D2：cert key 落本地磁盘 + 路径写 DB

每个 cert 一对 `(cert_pem, key_pem)`；cert_pem 落 DB（公开，所有 router 节点都能读），key_pem 落 `acme.key_storage_dir/<domain>.key.pem`（每节点本地）。多 router 节点部署：用户需要在所有节点的同一路径挂同一份 key（或后续接 secrets manager）。MVP 假设单 router 节点；多节点写文档说明限制。

替代方案：key 也落 DB。拒：key 是敏感 secret，落 DB 等于让 DBA 看到，违反"最少特权"。

### D3：用 axum-server + tls-rustls，不切 axum::serve

`axum::serve` 不支持 SNI 动态 cert resolver。`axum-server::bind_rustls` 支持自定义 `Arc<dyn ResolvesServerCert>`，最低改动让现有 router 一行接入 HTTPS。

### D4：80 端口仅 challenge + healthz + redirect

行业惯例：80 仅做 HTTP-01 challenge + 健康检查 + 301 → https。所有 API 走 443。MVP 不实装 HSTS（Strict-Transport-Security header）以避免开发期被卡住，但 spec 留口子。

### D5：renewal 在 background loop 里串行做

每个域单独发证可能要几秒（ACME order + challenge + finalize）；不并行避免 LE rate-limit 触发。50 个 domain 一轮约 100s，可接受。后续 follow-up 引入 `Semaphore(5)` 适度并行。

### D6：SNI resolver cache TTL 60s

域很少，cache 大小 = 域数；TTL 60s 让 cert 更新平均 30s 内生效。failure（unknown SNI）不缓存避免 thrashing。

## Risks / Trade-offs

**[R1] Let's Encrypt rate limit**：每 week 每 domain 50 个 cert；renewal 失败死循环会撞。
→ Mitigation：`AcmeClient::issue` 失败时 `domains.last_error` 写完后 60s 内不再重试（DB 字段 `next_attempt_at_micros` 留 follow-up；本 change 用进程内 `DashMap<domain_id, last_attempt>` 60s cooldown）。

**[R2] HTTP-01 要求 80 端口可从公网访问**：客户在 NAT / firewall 后会失败。
→ Mitigation：spec 文档明示前置条件；提供 `acme.directory = "pebble"` 模式让本地开发不依赖公网。

**[R3] cert key 多节点不同步**：multi-router 场景一个节点能 serve 另一个不能。
→ Mitigation：先文档说明单节点限制；同步通过共享存储（NFS / object_store + sym-link）由 ops 决定，本 change 不引入额外抽象。

**[R4] rustls 升级 break**：rustls 0.23 之后 API 变动频繁。
→ Mitigation：把 rustls 版本 pin 到 workspace 一行，跨 crate 共享；future migration 一次性升。

## Migration Plan

1. 部署带 `[http.tls].enabled=false` 的版本（与现状等价）
2. ops 在 DNS 把 `obs.acme.com → ip` 配好
3. POST `/api/v1/domains { hostname: "obs.acme.com" }`
4. 等 1-2 分钟，监控 `domains.state == active`
5. 切配置 `[http.tls].enabled=true` 重启
6. 验 `curl https://obs.acme.com/api/v1/healthz`

回滚：把 `tls.enabled` 改回 false 重启即可。
