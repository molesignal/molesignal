// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SAML 2.0 SP。
//!
//! 实装范围：
//! - `build_authz_url`：构造 AuthnRequest XML → DEFLATE → base64 → URL-encode（HTTP-Redirect binding）；
//! - `parse_response`：base64 decode + XML 解析提取 NameID/email/groups；
//! - **签名校验**：调 [`super::xmldsig::verify_assertion_signature`]，对 Assertion
//!   做 enveloped signature RSA-SHA256 verify（Phase 3b 实装）。
//!
//! XMLDSig 实装的 C14N 简化方式见 `xmldsig.rs` 的模块 doc — 对主流 IdP
//! （Azure AD / Okta / Keycloak / ADFS）输出能稳定通过；改格式的 Response 会
//! 主动拒绝。这是 raw-bytes 路径的副作用保护。

use std::{collections::BTreeMap, io::Write};

use base64::{Engine as _, engine::general_purpose};
use flate2::{Compression, write::DeflateEncoder};
use rand::TryRng as _;
use roxmltree::Document;
use serde::{Deserialize, Serialize};

use crate::{
    domain::iam::SsoFieldMapping,
    shared::{Error, Result, time::TimestampMicros},
};

/// 旧 `SsoConfig` 的兼容 alias —— 实际配置走 `crate::domain::iam::SsoSamlConfig`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SamlConfig {
    pub sp_entity_id: String,
    pub idp_entity_id: String,
    pub idp_sso_url: String,
    pub idp_x509_cert: String,
    pub assertion_consumer_url: String,
    pub field_mapping: SsoFieldMapping,
}

/// 提取自 SAML Response 的 user identity。
#[derive(Debug, Clone)]
pub struct SamlAssertion {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
    pub groups: Vec<String>,
}

pub struct SamlLoginFlow {
    cfg: SamlConfig,
}

impl SamlLoginFlow {
    pub fn new(cfg: SamlConfig) -> Self {
        Self { cfg }
    }

    /// 构造 HTTP-Redirect binding 的 AuthnRequest URL。
    ///
    /// 步骤：拼 XML → DEFLATE 无 wrapper → base64 → URL-encode → 拼 `idp_sso_url?SAMLRequest=...&RelayState=...`
    pub fn build_authz_url(&self, relay_state: &str) -> Result<String> {
        if self.cfg.idp_sso_url.is_empty() || self.cfg.sp_entity_id.is_empty() {
            return Err(Error::invalid(
                "saml: idp_sso_url and sp_entity_id required",
            ));
        }
        let request_id = format!("_{}", rand_hex(16));
        let issue_instant =
            chrono::DateTime::<chrono::Utc>::from_timestamp_micros(TimestampMicros::now().0)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());

        let xml = format!(
            r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="{id}" Version="2.0" IssueInstant="{ts}" Destination="{dest}" ProtocolBinding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" AssertionConsumerServiceURL="{acs}"><saml:Issuer>{sp}</saml:Issuer><samlp:NameIDPolicy Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress" AllowCreate="true"/></samlp:AuthnRequest>"#,
            id = request_id,
            ts = issue_instant,
            dest = xml_escape(&self.cfg.idp_sso_url),
            acs = xml_escape(&self.cfg.assertion_consumer_url),
            sp = xml_escape(&self.cfg.sp_entity_id),
        );

        // RFC 1951 raw DEFLATE（no zlib header），SAML spec 要求的形式
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(xml.as_bytes())
            .map_err(|e| Error::internal(format!("saml deflate: {e}")))?;
        let compressed = encoder
            .finish()
            .map_err(|e| Error::internal(format!("saml deflate finish: {e}")))?;
        let b64 = general_purpose::STANDARD.encode(&compressed);
        let encoded_request = url_encode(&b64);
        let encoded_relay = url_encode(relay_state);

        let sep = if self.cfg.idp_sso_url.contains('?') {
            "&"
        } else {
            "?"
        };
        Ok(format!(
            "{}{sep}SAMLRequest={encoded_request}&RelayState={encoded_relay}",
            self.cfg.idp_sso_url
        ))
    }

    /// 解 HTTP-POST binding 回调的 `SAMLResponse` 字段（base64-encoded SAML Response XML）。
    pub fn parse_response(&self, saml_response_b64: &str) -> Result<SamlAssertion> {
        let xml_bytes = general_purpose::STANDARD
            .decode(saml_response_b64.trim())
            .map_err(|e| Error::unauthorized(format!("saml b64: {e}")))?;
        // 严格 XMLDSig 验签：见 xmldsig.rs（raw-bytes 路径 + exclusive C14N 回退）。
        self.verify_signature(&xml_bytes)?;
        // 验签通过后再做 SP 侧条件校验（有效期 / audience / recipient）。
        self.validate_conditions(&xml_bytes)?;
        self.extract_assertion_data(&xml_bytes)
    }

    /// 仅解析 Assertion 数据（不验签）—— 测试 + 内部使用。
    /// 生产路径必须经 [`Self::parse_response`]，那条路径先验签后才调本方法。
    pub(super) fn extract_assertion_data(&self, xml_bytes: &[u8]) -> Result<SamlAssertion> {
        let xml_text = std::str::from_utf8(xml_bytes)
            .map_err(|e| Error::unauthorized(format!("saml utf8: {e}")))?;
        let doc =
            Document::parse(xml_text).map_err(|e| Error::unauthorized(format!("saml xml: {e}")))?;

        let root = doc.root_element();
        let assertion = root
            .descendants()
            .find(|n| n.tag_name().name() == "Assertion")
            .ok_or_else(|| Error::unauthorized("saml: no <Assertion>".to_string()))?;

        let name_id = assertion
            .descendants()
            .find(|n| n.tag_name().name() == "NameID")
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_owned();

        let mut attributes = BTreeMap::<String, Vec<String>>::new();
        for attr in assertion
            .descendants()
            .filter(|n| n.tag_name().name() == "Attribute")
        {
            let attr_name = attr.attribute("Name").unwrap_or("").trim();
            if attr_name.is_empty() {
                continue;
            }
            let values = attr
                .children()
                .filter(|c| c.tag_name().name() == "AttributeValue")
                .filter_map(|c| c.text())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if !values.is_empty() {
                attributes
                    .entry(attr_name.to_owned())
                    .or_default()
                    .extend(values);
            }
        }

        let subject = mapped_saml_values(
            &self.cfg.field_mapping.subject,
            &name_id,
            &attributes,
            &["NameID"],
        )
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::unauthorized(format!(
                "SAML identity is missing mapped subject field `{}`",
                self.cfg.field_mapping.subject
            ))
        })?;
        let mut email_values = mapped_saml_values(
            &self.cfg.field_mapping.email,
            &name_id,
            &attributes,
            &[
                "email",
                "mail",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress",
            ],
        );
        if email_values.is_empty()
            && self.cfg.field_mapping.email == "email"
            && name_id.contains('@')
        {
            email_values.push(name_id.clone());
        }
        let email = email_values.into_iter().next().ok_or_else(|| {
            Error::unauthorized(format!(
                "SAML identity is missing mapped email field `{}`",
                self.cfg.field_mapping.email
            ))
        })?;
        let name = mapped_saml_values(
            &self.cfg.field_mapping.display_name,
            &name_id,
            &attributes,
            &[
                "name",
                "displayName",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name",
            ],
        )
        .into_iter()
        .next();
        let mut groups = mapped_saml_values(
            &self.cfg.field_mapping.groups,
            &name_id,
            &attributes,
            &["groups", "memberOf", "Role", "Roles"],
        );
        groups.sort();
        groups.dedup();

        Ok(SamlAssertion {
            subject,
            email,
            name,
            groups,
        })
    }

    /// SP 侧条件校验（验签**之后**调用，故这些断言已确保来自可信 IdP 且未被篡改）：
    /// - `Conditions` 有效期窗口 `NotBefore` / `NotOnOrAfter`（±120s 时钟偏移容忍）；
    /// - `AudienceRestriction`：若存在，必须含本 SP 的 `sp_entity_id`（防 token 被复用到别的 SP）；
    /// - `SubjectConfirmationData`：若存在，校验 `NotOnOrAfter` 未过期、`Recipient` 匹配 ACS。
    fn validate_conditions(&self, xml_bytes: &[u8]) -> Result<()> {
        let xml_text = std::str::from_utf8(xml_bytes)
            .map_err(|e| Error::unauthorized(format!("saml utf8: {e}")))?;
        let doc =
            Document::parse(xml_text).map_err(|e| Error::unauthorized(format!("saml xml: {e}")))?;
        let assertion = doc
            .root_element()
            .descendants()
            .find(|n| n.tag_name().name() == "Assertion")
            .ok_or_else(|| Error::unauthorized("saml: no <Assertion>".to_string()))?;

        let now = TimestampMicros::now().0;
        const SKEW_US: i64 = 120_000_000; // 容忍 ±120s 时钟偏移

        if let Some(cond) = assertion
            .descendants()
            .find(|n| n.tag_name().name() == "Conditions")
        {
            if let Some(nb) = cond.attribute("NotBefore").and_then(parse_saml_time)
                && now + SKEW_US < nb
            {
                return Err(Error::unauthorized(
                    "saml: assertion not yet valid (Conditions/NotBefore)".to_string(),
                ));
            }
            if let Some(na) = cond.attribute("NotOnOrAfter").and_then(parse_saml_time)
                && now - SKEW_US >= na
            {
                return Err(Error::unauthorized(
                    "saml: assertion expired (Conditions/NotOnOrAfter)".to_string(),
                ));
            }
            let audiences: Vec<String> = cond
                .descendants()
                .filter(|n| n.tag_name().name() == "Audience")
                .filter_map(|n| n.text().map(|s| s.trim().to_string()))
                .collect();
            if !audiences.is_empty()
                && !self.cfg.sp_entity_id.is_empty()
                && !audiences.iter().any(|a| a == &self.cfg.sp_entity_id)
            {
                return Err(Error::unauthorized(
                    "saml: audience restriction does not include this SP".to_string(),
                ));
            }
        }

        if let Some(scd) = assertion
            .descendants()
            .find(|n| n.tag_name().name() == "SubjectConfirmationData")
        {
            if let Some(na) = scd.attribute("NotOnOrAfter").and_then(parse_saml_time)
                && now - SKEW_US >= na
            {
                return Err(Error::unauthorized(
                    "saml: subject confirmation expired".to_string(),
                ));
            }
            if let Some(recipient) = scd.attribute("Recipient")
                && !self.cfg.assertion_consumer_url.is_empty()
                && recipient != self.cfg.assertion_consumer_url
            {
                return Err(Error::unauthorized(
                    "saml: subject confirmation recipient mismatch".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// XMLDSig 真验签 — 调 [`super::xmldsig::verify_assertion_signature`]。
    ///
    /// 实装范围与限制见 `xmldsig.rs` 模块 doc：RSA-SHA256 + raw-bytes Reference digest，
    /// 不做 Exclusive C14N。主流 IdP 输出能通过；改 XML 格式的 Response 主动拒绝。
    fn verify_signature(&self, xml_bytes: &[u8]) -> Result<()> {
        if self.cfg.idp_x509_cert.trim().is_empty() {
            return Err(Error::unauthorized(
                "saml: idp_x509_cert not configured".to_string(),
            ));
        }
        super::xmldsig::verify_assertion_signature(xml_bytes, &self.cfg.idp_x509_cert)
    }
}

fn mapped_saml_values(
    field: &str,
    name_id: &str,
    attributes: &BTreeMap<String, Vec<String>>,
    default_aliases: &[&str],
) -> Vec<String> {
    if field.eq_ignore_ascii_case("NameID") {
        return if name_id.is_empty() {
            Vec::new()
        } else {
            vec![name_id.to_owned()]
        };
    }
    let names = std::iter::once(field).chain(
        default_aliases
            .first()
            .filter(|default| field == **default)
            .into_iter()
            .flat_map(|_| default_aliases.iter().copied().skip(1)),
    );
    names
        .filter_map(|name| attributes.get(name))
        .flatten()
        .cloned()
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 仅 percent-encode 必要字符；SAML 兼容性优先，多余字符也编一遍。
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn rand_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::rngs::SysRng.try_fill_bytes(&mut buf).expect("os rng");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 解析 SAML 时间戳（xsd:dateTime / RFC3339，含可选小数秒）为 epoch 微秒。
fn parse_saml_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|dt| dt.timestamp_micros())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SamlConfig {
        SamlConfig {
            sp_entity_id: "https://sp.local".into(),
            idp_entity_id: "https://idp.example/entity".into(),
            idp_sso_url: "https://idp.example/sso".into(),
            idp_x509_cert: "-----BEGIN CERTIFICATE-----\nABCD\n-----END CERTIFICATE-----".into(),
            assertion_consumer_url: "https://sp.local/api/v1/auth/sso/saml/callback".into(),
            field_mapping: SsoFieldMapping::saml(),
        }
    }

    #[test]
    fn build_authz_url_contains_required_params() {
        let f = SamlLoginFlow::new(cfg());
        let url = f.build_authz_url("relay_abc").unwrap();
        assert!(url.starts_with("https://idp.example/sso?SAMLRequest="));
        assert!(url.contains("RelayState=relay_abc"));
    }

    #[test]
    fn extract_assertion_data_pulls_email_and_groups() {
        // 不走 verify_signature — 那条路径由 xmldsig 单测覆盖（happy_path /
        // tampered_assertion / wrong_key）。这里只测属性提取逻辑。
        let xml = r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
  <saml:Assertion>
    <saml:Subject><saml:NameID>alice@example.com</saml:NameID></saml:Subject>
    <saml:AttributeStatement>
      <saml:Attribute Name="email"><saml:AttributeValue>alice@example.com</saml:AttributeValue></saml:Attribute>
      <saml:Attribute Name="groups">
        <saml:AttributeValue>g-admin</saml:AttributeValue>
        <saml:AttributeValue>g-eng</saml:AttributeValue>
      </saml:Attribute>
    </saml:AttributeStatement>
  </saml:Assertion>
</samlp:Response>"#;
        let a = SamlLoginFlow::new(cfg())
            .extract_assertion_data(xml.as_bytes())
            .unwrap();
        assert_eq!(a.email, "alice@example.com");
        assert_eq!(a.groups, vec!["g-admin", "g-eng"]);
    }

    #[test]
    fn custom_attribute_mapping_is_used() {
        let xml = r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
  <saml:Assertion>
    <saml:Subject><saml:NameID>legacy-id</saml:NameID></saml:Subject>
    <saml:AttributeStatement>
      <saml:Attribute Name="uid"><saml:AttributeValue>u-1</saml:AttributeValue></saml:Attribute>
      <saml:Attribute Name="mailAddress"><saml:AttributeValue>alice@example.com</saml:AttributeValue></saml:Attribute>
      <saml:Attribute Name="display"><saml:AttributeValue>Alice</saml:AttributeValue></saml:Attribute>
      <saml:Attribute Name="roles"><saml:AttributeValue>operator</saml:AttributeValue></saml:Attribute>
    </saml:AttributeStatement>
  </saml:Assertion>
</samlp:Response>"#;
        let mut config = cfg();
        config.field_mapping = SsoFieldMapping {
            subject: "uid".into(),
            email: "mailAddress".into(),
            display_name: "display".into(),
            groups: "roles".into(),
        };
        let assertion = SamlLoginFlow::new(config)
            .extract_assertion_data(xml.as_bytes())
            .expect("mapped SAML assertion");
        assert_eq!(assertion.subject, "u-1");
        assert_eq!(assertion.groups, ["operator"]);
    }

    fn assertion_with(conditions: &str, subject_conf: &str) -> String {
        format!(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">
  <saml:Assertion>
    <saml:Subject><saml:NameID>alice@example.com</saml:NameID>{subject_conf}</saml:Subject>
    {conditions}
  </saml:Assertion>
</samlp:Response>"#
        )
    }

    #[test]
    fn validate_conditions_accepts_valid_window_and_audience() {
        let f = SamlLoginFlow::new(cfg());
        let xml = assertion_with(
            r#"<saml:Conditions NotBefore="2000-01-01T00:00:00Z" NotOnOrAfter="2999-01-01T00:00:00Z">
                 <saml:AudienceRestriction><saml:Audience>https://sp.local</saml:Audience></saml:AudienceRestriction>
               </saml:Conditions>"#,
            "",
        );
        f.validate_conditions(xml.as_bytes())
            .expect("valid conditions");
    }

    #[test]
    fn validate_conditions_rejects_expired() {
        let f = SamlLoginFlow::new(cfg());
        let xml = assertion_with(
            r#"<saml:Conditions NotOnOrAfter="2000-01-01T00:00:00Z"/>"#,
            "",
        );
        let err = f.validate_conditions(xml.as_bytes()).expect_err("expired");
        assert!(format!("{err}").contains("expired"));
    }

    #[test]
    fn validate_conditions_rejects_wrong_audience() {
        let f = SamlLoginFlow::new(cfg());
        let xml = assertion_with(
            r#"<saml:Conditions NotOnOrAfter="2999-01-01T00:00:00Z">
                 <saml:AudienceRestriction><saml:Audience>https://other.sp</saml:Audience></saml:AudienceRestriction>
               </saml:Conditions>"#,
            "",
        );
        let err = f.validate_conditions(xml.as_bytes()).expect_err("audience");
        assert!(format!("{err}").contains("audience"));
    }

    #[test]
    fn validate_conditions_rejects_recipient_mismatch() {
        let f = SamlLoginFlow::new(cfg());
        let xml = assertion_with(
            r#"<saml:Conditions NotOnOrAfter="2999-01-01T00:00:00Z"/>"#,
            r#"<saml:SubjectConfirmation><saml:SubjectConfirmationData NotOnOrAfter="2999-01-01T00:00:00Z" Recipient="https://attacker.example/acs"/></saml:SubjectConfirmation>"#,
        );
        let err = f
            .validate_conditions(xml.as_bytes())
            .expect_err("recipient");
        assert!(format!("{err}").contains("recipient"));
    }
}
