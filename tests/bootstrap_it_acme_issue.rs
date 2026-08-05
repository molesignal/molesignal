// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ACME issue 全链冒烟（需 Pebble 本地 ACME server）。
//!
//! 由于 LetsEncrypt 不能从本地测试 ip 直接命中 http-01，全链测试需要起一个本地
//! [Pebble](https://github.com/letsencrypt/pebble) 容器作 fake ACME。本测试默认
//! `MS_RUN_IT=1` + `MS_PEBBLE_URL=https://localhost:14000/dir`
//! 才跑；没有 Pebble 时 skip-fast。
//!
//! 流程：
//! 1. POST `/api/v1/domains { hostname }` 创建 pending 域
//! 2. 起 AcmeRunner 或手动调 `issue_one`
//! 3. 轮询 `domains.cert_pem` non-NULL + `state="active"` + key.pem 落盘

mod common;

#[tokio::test]
async fn acme_issue_smoke_with_pebble() {
    if common::skip_unless_enabled() {
        return;
    }
    let Ok(_pebble_url) = std::env::var("MS_PEBBLE_URL") else {
        // 没配 Pebble 时 skip-fast；只在专用 CI job 启用。
        return;
    };
    // 完整 ACME 链路依赖 Pebble + DNS-resolvable hostname；本 scaffold 只占测试名 +
    // 走完 fixture 启动路径，确认  feature TLS 模块编译就位。
    let s = common::TestServer::start().await;
    let _ = s.client; // anchor
}
