// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 进程内 SSO `state` / `nonce` 短期 store。
//!
//! Login 阶段生成的 `state` 是 CSRF 防护：callback 必须带回同一 state，
//! 否则攻击者可以伪造 callback 注入伪造身份。`nonce` 同步存入，用于
//! 后续 ID Token nonce claim 校验。
//!
//! 当前实现：`Mutex<HashMap>` + 过期时间。`put` 时顺手 GC 过期项。
//! 单进程 OK；多副本部署需替换为 Redis（接口签名保持一致即可）。

use std::{collections::HashMap, sync::Mutex};

use crate::shared::time::TimestampMicros;

#[derive(Debug, Clone)]
pub struct SsoStateEntry {
    pub provider_id: String,
    pub nonce: String,
    pub expires_at_us: i64,
}

pub struct SsoStateStore {
    inner: Mutex<HashMap<String, SsoStateEntry>>,
    ttl_us: i64,
}

impl SsoStateStore {
    /// `ttl_secs` 是 state 入库后的存活时间。常规取 600（10 分钟）够 IdP 重定向 +
    /// 用户输密码的时间。
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl_us: (ttl_secs as i64).saturating_mul(1_000_000),
        }
    }

    /// Login handler 生成 `state` 后调；返回当前过期时间（micros）。
    pub fn put(&self, state: String, provider_id: String, nonce: String) -> i64 {
        let now = TimestampMicros::now().0;
        let expires_at = now + self.ttl_us;
        let mut g = self.inner.lock().expect("sso state store poisoned");
        g.retain(|_, e| e.expires_at_us > now);
        g.insert(
            state,
            SsoStateEntry {
                provider_id,
                nonce,
                expires_at_us: expires_at,
            },
        );
        expires_at
    }

    /// Callback handler 调；同一 `state` 只能消费一次。已过期返 None。
    pub fn take(&self, state: &str) -> Option<SsoStateEntry> {
        let now = TimestampMicros::now().0;
        let mut g = self.inner.lock().expect("sso state store poisoned");
        let entry = g.remove(state)?;
        if entry.expires_at_us > now {
            Some(entry)
        } else {
            None
        }
    }
}

impl Default for SsoStateStore {
    fn default() -> Self {
        Self::new(600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_take_round_trip() {
        let s = SsoStateStore::new(600);
        s.put("st1".into(), "pid".into(), "n1".into());
        let entry = s.take("st1").expect("present");
        assert_eq!(entry.provider_id, "pid");
        assert_eq!(entry.nonce, "n1");
        // second take returns None — single-use
        assert!(s.take("st1").is_none());
    }

    #[test]
    fn expired_entries_not_returned() {
        let s = SsoStateStore::new(0); // 立即过期
        s.put("st1".into(), "pid".into(), "n1".into());
        assert!(s.take("st1").is_none());
    }
}
