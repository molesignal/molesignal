## Why

Domain management 已经接通（CRUD + ACME challenge endpoint，spec M1），但缺三块没有就只是个 placeholder：
- 没有真正的 ACME client（`acme_challenges` 表只能"被动响应"，没人发起 issue / renewal）
- 没有 TLS 服务端的 SNI cert selector，已签发证书无处可挂
- 没有后台 renewal loop

少了这三块，"自定义域名 + 自动 LetsEncrypt 证书"在 README 对比表上就是空话。

## What Changes

- 引入 `instant-acme` crate（纯 Rust ACME 客户端）。
- 新增 `AcmeClient` 实装：order → http-01 challenge → finalize → 拿 cert chain → 落 `domains.cert_pem` + `cert_not_after_micros`。
- 新增 `acme_runner` 后台 worker（与 search_jobs / scheduled_reports 一档）：
  - 每分钟扫 `state=pending` 的 domain → issue
  - 每 6h（`RENEWAL_RETRY_SECS`）扫 cert 30 天内到期 → renewal
  - 失败 backoff 写 `last_error` + 状态 `failed`
- 新增 `SniCertResolver`（实装 `rustls::server::ResolvesServerCert`）：从 `domains` 表实时查 `(SNI hostname → cert_pem)`，构 `rustls::sign::CertifiedKey`。
- HTTP server 在 wire 阶段切到 rustls 模式：`axum::serve` 改 `axum_server::bind_rustls` + 把 `SniCertResolver` 注入 `ServerConfig`。
- 同 socket 上 80 端口仍保留明文（HTTP-01 challenge endpoint + health），其他 443。

## Capabilities

### New Capabilities
<!-- 无 -->

### Modified Capabilities
- `domain-management`: 从"schema + challenge endpoint stub"升级为真实 ACME client + auto renewal + SNI hosting。

## Impact

- **依赖**：workspace 加 `instant-acme = "0.7"` + `rcgen` (test) + `rustls = "0.23"` + `tokio-rustls` + `axum-server = { features = ["tls-rustls"] }`。
- **配置**：`[http.tls]` 新增 `enabled` / `bind_addr` / `acme_directory` (`production` | `staging` | `pebble`) / `account_email` / `key_storage_dir`（缓存 ACME account key + cert key）。
- **wire**：`build_state` 后启动两套 server task —— 80 plain (api + acme challenge) + 443 rustls (api + sni cert resolver)；现有 `axum::serve` 路径在 `tls.enabled=false` 时保留为兼容。
- **新 worker**：`crates/bootstrap/src/workers/acme.rs`。
- **新 cert resolver**：`crates/bootstrap/src/tls/sni_resolver.rs`。
- **不动 OSS**：整个改造都在 `cfg(feature = "enterprise")` 下编译。
- **测试**：
  - unit：`SniCertResolver` 返已知 cert / fallback / 未知 SNI 返 None
  - integration（require pebble docker）：`it_acme_issue.rs` 全链发证一个 domain
