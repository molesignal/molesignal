// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Stripe 计费集成（入站：验签 + 订阅状态同步）。
//!
//! 纯函数层，不发出站请求：
//! - [`verify_stripe_signature`]：校验 `Stripe-Signature`（HMAC-SHA256 + 时间戳容差）。
//! - [`stripe_status_to_state`]：Stripe 订阅 status → 本地 [`SubscriptionState`]。
//! - [`parse_subscription_event`]：从 webhook 事件提取订阅快照。
//!
//! webhook 入口（api crate）验签后调 [`parse_subscription_event`]，把状态 upsert 进
//! `marketplace_subscriptions`（provider="stripe"），供 ingest 计费门禁读取。

use sha2::{Digest, Sha256};

use crate::{
    cloud_marketplace::SubscriptionState,
    shared::{Error, Result},
};

/// Stripe 计费 provider 名（写入 `marketplace_subscriptions.provider`）。
pub const STRIPE_PROVIDER: &str = "stripe";

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

/// HMAC-SHA256（RFC 2104）。手写避免 `hmac`/`digest` 版本与 `sha2 0.11` 冲突。
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut hi = Sha256::new();
    hi.update(ipad);
    hi.update(msg);
    let inner = hi.finalize();
    let mut ho = Sha256::new();
    ho.update(opad);
    ho.update(inner);
    let mut out = [0u8; 32];
    out.copy_from_slice(&ho.finalize());
    out
}

/// 定长常数时间比较，避免签名校验旁路计时攻击。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 校验 Stripe webhook 签名。`sig_header` 形如 `t=12345,v1=hex[,v1=hex...]`；
/// `signed_payload = "{t}.{raw_body}"`，期望 = `HMAC-SHA256(secret, signed_payload)` 的 hex。
/// `tolerance_secs > 0` 时校验 `|now - t| <= tolerance`（防重放）。
pub fn verify_stripe_signature(
    payload: &[u8],
    sig_header: &str,
    secret: &str,
    now_unix: i64,
    tolerance_secs: i64,
) -> Result<()> {
    if secret.is_empty() {
        return Err(Error::internal("stripe webhook secret not configured"));
    }
    let mut timestamp: Option<i64> = None;
    let mut v1_sigs: Vec<&str> = Vec::new();
    for part in sig_header.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.trim() {
            "t" => timestamp = v.trim().parse::<i64>().ok(),
            "v1" => v1_sigs.push(v.trim()),
            _ => {}
        }
    }
    let ts = timestamp.ok_or_else(|| Error::unauthorized("Stripe-Signature missing t"))?;
    if v1_sigs.is_empty() {
        return Err(Error::unauthorized("Stripe-Signature missing v1"));
    }
    if tolerance_secs > 0 && (now_unix - ts).abs() > tolerance_secs {
        return Err(Error::unauthorized(
            "Stripe-Signature timestamp outside tolerance",
        ));
    }
    let mut signed = Vec::with_capacity(payload.len() + 16);
    signed.extend_from_slice(ts.to_string().as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(payload);
    let expected = hex::encode(hmac_sha256(secret.as_bytes(), &signed));
    if v1_sigs
        .iter()
        .any(|s| ct_eq(s.as_bytes(), expected.as_bytes()))
    {
        Ok(())
    } else {
        Err(Error::unauthorized("Stripe-Signature verification failed"))
    }
}

/// Stripe 订阅 status → 本地状态机。
/// `active`/`trialing` 视为服务有效；`past_due`/`unpaid`/`incomplete`/`paused` 暂停
/// （需付款）；`canceled`/`incomplete_expired` 取消（终态）。
pub fn stripe_status_to_state(status: &str) -> SubscriptionState {
    match status {
        "active" | "trialing" => SubscriptionState::Active,
        "past_due" | "unpaid" | "incomplete" | "paused" => SubscriptionState::Suspended,
        "canceled" | "incomplete_expired" => SubscriptionState::Cancelled,
        _ => SubscriptionState::Pending,
    }
}

/// 从 webhook 事件提取的订阅快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeSubscriptionEvent {
    pub event_type: String,
    pub subscription_id: String,
    pub customer_id: String,
    pub status: String,
    pub price_id: Option<String>,
    /// 订阅创建时写入 metadata.org_id（绑定到 molesignal org）。
    pub org_id: Option<String>,
}

/// 解析 Stripe webhook 事件体。仅 `customer.subscription.*` 返回 `Some`；其它事件
/// （invoice/payment 等）返回 `None`（caller 应答 200 但不动状态）。
pub fn parse_subscription_event(body: &[u8]) -> Result<Option<StripeSubscriptionEvent>> {
    let v: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| Error::invalid(format!("stripe event json: {e}")))?;
    let event_type = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    if !event_type.starts_with("customer.subscription.") {
        return Ok(None);
    }
    let obj = v
        .pointer("/data/object")
        .ok_or_else(|| Error::invalid("stripe event missing data.object"))?;
    let subscription_id = obj
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    if subscription_id.is_empty() {
        return Err(Error::invalid("stripe subscription event missing id"));
    }
    let customer_id = obj
        .get("customer")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    // deleted 事件以 event_type 兜底为 canceled（payload.status 可能不可靠）。
    let status = if event_type == "customer.subscription.deleted" {
        "canceled".to_string()
    } else {
        obj.get("status")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let price_id = obj
        .pointer("/items/data/0/price/id")
        .and_then(|x| x.as_str())
        .map(String::from);
    let org_id = obj
        .pointer("/metadata/org_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    Ok(Some(StripeSubscriptionEvent {
        event_type,
        subscription_id,
        customer_id,
        status,
        price_id,
        org_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc4231_test_case_2() {
        // RFC 4231 test vector: key="Jefe", data="what do ya want for nothing?"。
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    fn sign(secret: &str, ts: i64, payload: &[u8]) -> String {
        let mut signed = ts.to_string().into_bytes();
        signed.push(b'.');
        signed.extend_from_slice(payload);
        format!(
            "t={ts},v1={}",
            hex::encode(hmac_sha256(secret.as_bytes(), &signed))
        )
    }

    #[test]
    fn verifies_valid_signature_within_tolerance() {
        let payload = br#"{"type":"customer.subscription.updated"}"#;
        let header = sign("whsec_test", 1_000, payload);
        assert!(verify_stripe_signature(payload, &header, "whsec_test", 1_100, 300).is_ok());
    }

    #[test]
    fn rejects_tampered_payload_and_wrong_secret() {
        let payload = br#"{"a":1}"#;
        let header = sign("whsec_test", 1_000, payload);
        // 篡改 body。
        assert!(verify_stripe_signature(b"{\"a\":2}", &header, "whsec_test", 1_000, 300).is_err());
        // 错误密钥。
        assert!(verify_stripe_signature(payload, &header, "whsec_other", 1_000, 300).is_err());
    }

    #[test]
    fn rejects_stale_timestamp() {
        let payload = br#"{}"#;
        let header = sign("whsec_test", 1_000, payload);
        // now 远超容差。
        assert!(verify_stripe_signature(payload, &header, "whsec_test", 9_999, 300).is_err());
    }

    #[test]
    fn status_mapping_covers_lifecycle() {
        assert_eq!(stripe_status_to_state("active"), SubscriptionState::Active);
        assert_eq!(
            stripe_status_to_state("trialing"),
            SubscriptionState::Active
        );
        assert_eq!(
            stripe_status_to_state("past_due"),
            SubscriptionState::Suspended
        );
        assert_eq!(
            stripe_status_to_state("canceled"),
            SubscriptionState::Cancelled
        );
        assert_eq!(stripe_status_to_state("weird"), SubscriptionState::Pending);
    }

    #[test]
    fn parses_subscription_event_with_metadata() {
        let body = br#"{
            "type": "customer.subscription.updated",
            "data": { "object": {
                "id": "sub_123", "object": "subscription", "customer": "cus_9",
                "status": "active",
                "items": { "data": [ { "price": { "id": "price_x" } } ] },
                "metadata": { "org_id": "org-7" }
            } }
        }"#;
        let ev = parse_subscription_event(body).unwrap().unwrap();
        assert_eq!(ev.subscription_id, "sub_123");
        assert_eq!(ev.customer_id, "cus_9");
        assert_eq!(ev.status, "active");
        assert_eq!(ev.price_id.as_deref(), Some("price_x"));
        assert_eq!(ev.org_id.as_deref(), Some("org-7"));
    }

    #[test]
    fn deleted_event_forces_canceled() {
        let body = br#"{"type":"customer.subscription.deleted","data":{"object":{"id":"sub_1","customer":"cus_1","status":"active"}}}"#;
        let ev = parse_subscription_event(body).unwrap().unwrap();
        assert_eq!(ev.status, "canceled");
    }

    #[test]
    fn ignores_non_subscription_events() {
        let body = br#"{"type":"invoice.payment_succeeded","data":{"object":{}}}"#;
        assert!(parse_subscription_event(body).unwrap().is_none());
    }
}
