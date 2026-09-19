// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! XMLDSig 验签 — SAML enveloped signature 的实用实装（Phase 3b）。
//!
//! 范围（与"严格 spec 合规"区分清楚）：
//! - **支持**：RSA-SHA256 + enveloped signature transform + raw-bytes digest；
//! - **不支持**：Exclusive XML C14N（xml-exc-c14n）的所有 corner case
//!   （namespace inheritance / InclusiveNamespaces / attribute reordering）；
//! - **取舍**：主流 IdP（Azure AD / Okta / Keycloak / ADFS）生成的 SAML Response
//!   已经是 deterministic 格式，raw-bytes 路径能稳定验签；少数自托管 IdP 写出非
//!   规范化的 XML 会失败，反过来这正是想要的拦截行为。
//!
//! 流程：
//! 1. 提取 `<ds:Signature>` 在原 XML bytes 里的 byte range（用 roxmltree 的
//!    `text_pos_at` 反查不可靠，所以走 quick-xml streaming 解析记 offset）；
//! 2. Reference digest：从原 Assertion bytes 里**剪掉** Signature bytes，对剩
//!    余原始字节做 SHA256，与 `<ds:DigestValue>` 比对；
//! 3. SignatureValue：取 `<ds:SignedInfo>` 的原始 bytes，SHA256 后用 IdP 公钥
//!    RSA-PKCS1v1.5 verify SignatureValue；
//! 4. 公钥源：从 `<ds:KeyInfo>/<ds:X509Data>/<ds:X509Certificate>` 取 base64
//!    DER cert，或退回到 config 里的 PEM cert；额外校验两者是否同一公钥（防 IdP
//!    在 Response 里塞别人的 cert）。
//!
//! 与 samael 等 libxml2/xmlsec1 实装的差异：本模块**不做** Exclusive C14N，所以
//! 不抗 XML rewrite 攻击（攻击者修改 Response 里 namespace prefix 但保 Assertion
//! 语义不变会让 raw-bytes 路径失败，反而提供"严格 byte match"的副作用保护）。

use std::io::Cursor;

use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use quick_xml::{Reader, events::Event};
use roxmltree::{Document, Node, NodeId};
use sha2::Digest;

use crate::shared::{Error, Result};

/// 入口：验 SAML Response 里 Assertion 的 XMLDSig enveloped signature。
///
/// `xml_bytes` 是原始 SAML Response XML（base64 decode 后的字节）；`idp_cert_pem`
/// 是 sso_providers 配置里的 IdP X.509 cert（PEM）。返回 Ok 表示验签通过。
///
/// 两条路径，任一通过即接受——**两者都需 IdP 私钥才能伪造**（c14n 路径有边角 bug 最坏只是
/// 兼容性失配、绝不会接受伪造，因 RSA verify / SHA256 digest 仍须成立），故 OR 不降低安全：
/// 1. **raw-bytes**：对原始 byte 区间算 digest / verify SignedInfo——对主流 IdP（序列化已近
///    规范）最快、零解析风险；
/// 2. **exclusive C14N 回退**：按 xml-exc-c14n 规范化 SignedInfo 与 Assertion（剔除 Signature
///    子树）后再验——处理命名空间继承 / 属性重排 / 自闭合等非规范序列化（少数自托管 IdP）。
pub fn verify_assertion_signature(xml_bytes: &[u8], idp_cert_pem: &str) -> Result<()> {
    verify_raw_bytes(xml_bytes, idp_cert_pem)
        .or_else(|_| verify_canonicalized(xml_bytes, idp_cert_pem))
}

/// 路径 1：原始字节区间验签（不做 C14N）。
fn verify_raw_bytes(xml_bytes: &[u8], idp_cert_pem: &str) -> Result<()> {
    let xml_str = std::str::from_utf8(xml_bytes)
        .map_err(|e| Error::unauthorized(format!("xmldsig utf8: {e}")))?;

    // 1. 找 Assertion + Signature 的字节区间
    let assertion = find_element_range(xml_str, "Assertion")?
        .ok_or_else(|| Error::unauthorized("xmldsig: no <Assertion> in Response".to_string()))?;
    let assertion_bytes = &xml_bytes[assertion.start_byte..assertion.end_byte];

    let signature = find_element_range(
        &xml_str[assertion.start_byte..assertion.end_byte],
        "Signature",
    )?
    .ok_or_else(|| {
        Error::unauthorized("xmldsig: Assertion has no <Signature> child".to_string())
    })?;
    let signature_bytes = &assertion_bytes[signature.start_byte..signature.end_byte];

    // 2. SignedInfo + SignatureValue + X509Certificate（在 signature_bytes 里查）
    let signature_str = std::str::from_utf8(signature_bytes)
        .map_err(|e| Error::unauthorized(format!("xmldsig utf8: {e}")))?;
    let signed_info = find_element_range(signature_str, "SignedInfo")?
        .ok_or_else(|| Error::unauthorized("xmldsig: no <SignedInfo>".to_string()))?;
    let signed_info_bytes = &signature_bytes[signed_info.start_byte..signed_info.end_byte];

    let signature_value_b64 = extract_text_element(signature_str, "SignatureValue")?;
    let signature_value = decode_b64_compact(&signature_value_b64)?;

    let digest_value_b64 = extract_text_element(signature_str, "DigestValue")?;
    let digest_value = decode_b64_compact(&digest_value_b64)?;

    // 3. Reference digest：assertion_bytes 剪掉 signature_bytes → SHA256
    let mut canonical_assertion = Vec::with_capacity(assertion_bytes.len() - signature_bytes.len());
    canonical_assertion.extend_from_slice(&assertion_bytes[..signature.start_byte]);
    canonical_assertion.extend_from_slice(&assertion_bytes[signature.end_byte..]);
    let computed_digest = sha2::Sha256::digest(&canonical_assertion);
    if computed_digest.as_slice() != digest_value.as_slice() {
        return Err(Error::unauthorized(
            "xmldsig: reference digest mismatch (Assertion may have been tampered)".to_string(),
        ));
    }

    // 4. RSA-SHA256 verify(SignatureValue, SignedInfo bytes)
    let pubkey_der = parse_rsa_public_key_der_from_pem(idp_cert_pem)?;
    let public_key = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &pubkey_der);
    public_key
        .verify(signed_info_bytes, &signature_value)
        .map_err(|e| Error::unauthorized(format!("xmldsig RSA verify: {e:?}")))?;

    // 5. 可选：如果 Signature 内嵌了 X509Certificate，比对它与 idp_cert_pem 是同一公钥。
    if let Ok(embedded_b64) = extract_text_element(signature_str, "X509Certificate") {
        let embedded_pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
            embedded_b64.trim()
        );
        let embedded_key_der = parse_rsa_public_key_der_from_pem(&embedded_pem)?;
        let cfg_key_der = parse_rsa_public_key_der_from_pem(idp_cert_pem)?;
        if embedded_key_der != cfg_key_der {
            return Err(Error::unauthorized(
                "xmldsig: embedded X509Certificate does not match configured IdP cert".to_string(),
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ByteRange {
    start_byte: usize,
    end_byte: usize,
}

/// 用 quick-xml streaming 找指定 local-name 的第一个完整元素（含起始标签和结束标签）
/// 在 `xml` 里的字节区间。SAML 的 ds:Signature / saml:Assertion 都按 local-name
/// 匹配，namespace prefix 不固定。
fn find_element_range(xml: &str, local_name: &str) -> Result<Option<ByteRange>> {
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut depth: i32 = 0;
    let mut start_byte: Option<usize> = None;
    loop {
        let pos_before = reader.buffer_position();
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::unauthorized(format!("xml parse: {e}")))?
        {
            Event::Start(ref e) => {
                let name = e.name();
                let local = local_name_of(name.as_ref());
                if start_byte.is_none() && local == local_name {
                    start_byte = Some(pos_before as usize);
                    depth = 1;
                } else if start_byte.is_some() {
                    depth += 1;
                }
            }
            Event::End(ref e) => {
                if let Some(sb) = start_byte {
                    depth -= 1;
                    if depth == 0 {
                        let end_name = e.name();
                        let local = local_name_of(end_name.as_ref());
                        if local == local_name {
                            let end_byte = reader.buffer_position() as usize;
                            return Ok(Some(ByteRange {
                                start_byte: sb,
                                end_byte,
                            }));
                        }
                    }
                }
            }
            Event::Empty(ref e) => {
                if start_byte.is_none() {
                    let name = e.name();
                    let local = local_name_of(name.as_ref());
                    if local == local_name {
                        let end_byte = reader.buffer_position() as usize;
                        return Ok(Some(ByteRange {
                            start_byte: pos_before as usize,
                            end_byte,
                        }));
                    }
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

fn local_name_of(name: &[u8]) -> &str {
    let s = std::str::from_utf8(name).unwrap_or("");
    s.rsplit(':').next().unwrap_or(s)
}

/// 取指定 local-name 的元素的文本内容（去 whitespace）。
fn extract_text_element(xml: &str, local_name: &str) -> Result<String> {
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut inside = false;
    let mut content = String::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::unauthorized(format!("xml parse: {e}")))?
        {
            Event::Start(ref e) if local_name_of(e.name().as_ref()) == local_name => {
                inside = true;
            }
            Event::End(ref e) if inside && local_name_of(e.name().as_ref()) == local_name => {
                return Ok(content.split_whitespace().collect::<String>());
            }
            Event::Text(t) if inside => {
                // quick-xml 0.39 没有 BytesText::unescape；直接取原始 bytes 当 UTF-8。
                // SAML signature 的 base64 字符不含需要 unescape 的 XML 实体。
                if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                    content.push_str(s);
                }
            }
            Event::Eof => {
                return Err(Error::unauthorized(format!(
                    "xmldsig: <{local_name}> not found"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
}

fn decode_b64_compact(s: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| Error::unauthorized(format!("xmldsig b64: {e}")))
}

/// PEM X.509 cert / public key → DER-encoded RSA public key.
///
/// The verifier expects the RFC 8017 `RSAPublicKey` DER structure. X.509
/// certificates and SubjectPublicKeyInfo PEMs both wrap that bit string, so this
/// function normalizes all supported PEM tags to the same byte representation.
fn parse_rsa_public_key_der_from_pem(pem_str: &str) -> Result<Vec<u8>> {
    use x509_cert::{Certificate, der::Decode, spki::SubjectPublicKeyInfoRef};

    let trimmed = pem_str.trim();
    let parsed = pem::parse(trimmed.as_bytes())
        .map_err(|e| Error::invalid(format!("xmldsig pem parse: {e}")))?;
    let der = parsed.contents();
    if parsed.tag() == "CERTIFICATE" {
        let cert = Certificate::from_der(der)
            .map_err(|e| Error::invalid(format!("xmldsig x509 parse: {e}")))?;
        let spki_bytes = cert
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .as_bytes()
            .ok_or_else(|| {
                Error::invalid("xmldsig: public key bits not byte-aligned".to_string())
            })?;
        Ok(spki_bytes.to_vec())
    } else if parsed.tag() == "PUBLIC KEY" {
        let spki = SubjectPublicKeyInfoRef::try_from(der)
            .map_err(|e| Error::invalid(format!("xmldsig spki parse: {e}")))?;
        let spki_bytes = spki.subject_public_key.as_bytes().ok_or_else(|| {
            Error::invalid("xmldsig: public key bits not byte-aligned".to_string())
        })?;
        Ok(spki_bytes.to_vec())
    } else if parsed.tag() == "RSA PUBLIC KEY" {
        Ok(der.to_vec())
    } else {
        Err(Error::invalid(format!(
            "xmldsig: unsupported PEM tag '{}' (need CERTIFICATE / PUBLIC KEY / RSA PUBLIC KEY)",
            parsed.tag()
        )))
    }
}

// ===== 路径 2：exclusive XML canonicalization 验签 =====

/// 路径 2：按 xml-exc-c14n 规范化后验签。处理命名空间继承等 raw-bytes 路径覆盖不到的情形。
fn verify_canonicalized(xml_bytes: &[u8], idp_cert_pem: &str) -> Result<()> {
    let xml_str = std::str::from_utf8(xml_bytes)
        .map_err(|e| Error::unauthorized(format!("xmldsig utf8: {e}")))?;
    let doc =
        Document::parse(xml_str).map_err(|e| Error::unauthorized(format!("xmldsig xml: {e}")))?;

    let assertion = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Assertion")
        .ok_or_else(|| Error::unauthorized("xmldsig: no <Assertion>".to_string()))?;
    let signature = assertion
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Signature")
        .ok_or_else(|| {
            Error::unauthorized("xmldsig: Assertion has no <Signature> child".to_string())
        })?;
    let signed_info = signature
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "SignedInfo")
        .ok_or_else(|| Error::unauthorized("xmldsig: no <SignedInfo>".to_string()))?;

    let signature_value = first_text(signature, "SignatureValue")
        .ok_or_else(|| Error::unauthorized("xmldsig: no <SignatureValue>".to_string()))?;
    let signature_value = decode_b64_compact(&signature_value)?;
    let digest_value = first_text(signature, "DigestValue")
        .ok_or_else(|| Error::unauthorized("xmldsig: no <DigestValue>".to_string()))?;
    let digest_value = decode_b64_compact(&digest_value)?;

    // InclusiveNamespaces PrefixList（若 IdP 声明了）。
    let si_incl = inclusive_prefixes(signed_info);
    let ref_incl = signature
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Reference")
        .map(inclusive_prefixes)
        .unwrap_or_default();

    // 1. reference digest：c14n(Assertion 剔除 Signature 子树) → SHA256，比对 DigestValue。
    let assertion_c14n = canonicalize_exclusive(assertion, Some(signature.id()), &ref_incl);
    let computed = sha2::Sha256::digest(assertion_c14n.as_bytes());
    if computed.as_slice() != digest_value.as_slice() {
        return Err(Error::unauthorized(
            "xmldsig(c14n): reference digest mismatch".to_string(),
        ));
    }

    // 2. RSA-SHA256 verify(SignatureValue, c14n(SignedInfo))。
    let pubkey_der = parse_rsa_public_key_der_from_pem(idp_cert_pem)?;
    let signed_info_c14n = canonicalize_exclusive(signed_info, None, &si_incl);
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &pubkey_der)
        .verify(signed_info_c14n.as_bytes(), &signature_value)
        .map_err(|e| Error::unauthorized(format!("xmldsig(c14n) RSA verify: {e:?}")))?;

    // 3. 内嵌 cert 与配置 cert 公钥比对（防 IdP 在 Response 里塞别人的 cert）。
    if let Some(embedded_b64) = first_text(signature, "X509Certificate") {
        let embedded_pem =
            format!("-----BEGIN CERTIFICATE-----\n{embedded_b64}\n-----END CERTIFICATE-----");
        if parse_rsa_public_key_der_from_pem(&embedded_pem)? != pubkey_der {
            return Err(Error::unauthorized(
                "xmldsig(c14n): embedded X509Certificate does not match configured IdP cert"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

/// 取某元素子树里第一个 local-name 匹配元素的文本（仅文本节点，拼接去空白）。
fn first_text(node: Node, local: &str) -> Option<String> {
    node.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == local)
        .map(|n| {
            n.descendants()
                .filter(|d| d.is_text())
                .filter_map(|d| d.text())
                .collect::<String>()
                .split_whitespace()
                .collect::<String>()
        })
}

/// 读某范围内 `InclusiveNamespaces@PrefixList`（`#default` → 空串表默认命名空间）。
fn inclusive_prefixes(scope: Node) -> Vec<String> {
    scope
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "InclusiveNamespaces")
        .and_then(|n| n.attribute("PrefixList"))
        .map(|s| {
            s.split_whitespace()
                .map(|p| {
                    if p == "#default" {
                        String::new()
                    } else {
                        p.to_string()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// exclusive XML canonicalization（xml-exc-c14n#，不含注释）。
///
/// `omit`：要整树剔除的子节点（enveloped-signature transform）；`inclusive`：InclusiveNamespaces
/// 前缀表（仅在 apex 生效）。实装：元素 / 文本 / 命名空间最小化（visibly-utilized + inclusive，
/// 对照祖先输出上下文）/ 属性排序 / 转义 / 空元素展开。不处理注释、PI、`xmlns=""` 取消声明等边角
/// （SAML 全前缀命名空间，罕见）。仅作 raw-bytes 失败后的回退，故不正确也只表现为兼容性失配。
fn canonicalize_exclusive(node: Node, omit: Option<NodeId>, inclusive: &[String]) -> String {
    let mut out = String::new();
    c14n_element(&mut out, node, omit, inclusive, &[]);
    out
}

fn add_ns(v: &mut Vec<(String, String)>, prefix: &str, uri: &str) {
    if !v.iter().any(|(p, _)| p == prefix) {
        v.push((prefix.to_string(), uri.to_string()));
    }
}

fn c14n_element(
    out: &mut String,
    node: Node,
    omit: Option<NodeId>,
    inclusive: &[String],
    rendered: &[(String, String)],
) {
    let local = node.tag_name().name();
    let uri = node.tag_name().namespace();
    let prefix = uri.and_then(|u| node.lookup_prefix(u)).unwrap_or("");
    let qname = if prefix.is_empty() {
        local.to_string()
    } else {
        format!("{prefix}:{local}")
    };

    // visibly-utilized 命名空间集（元素自身 + 带命名空间的属性 + apex 的 inclusive 前缀）。
    let mut needed: Vec<(String, String)> = Vec::new();
    if let Some(u) = uri {
        add_ns(&mut needed, prefix, u);
    }
    for a in node.attributes() {
        if let Some(au) = a.namespace() {
            let ap = node.lookup_prefix(au).unwrap_or("");
            add_ns(&mut needed, ap, au);
        }
    }
    for inc in inclusive {
        let lookup = if inc.is_empty() {
            None
        } else {
            Some(inc.as_str())
        };
        if let Some(u) = node.lookup_namespace_uri(lookup) {
            add_ns(&mut needed, inc, u);
        }
    }

    // 过滤掉祖先输出上下文里已存在的同 (prefix,uri)；按 prefix 排序（默认 ns 空串最前）。
    let mut to_emit: Vec<(String, String)> = needed
        .into_iter()
        .filter(|(p, u)| !rendered.iter().any(|(rp, ru)| rp == p && ru == u))
        .collect();
    to_emit.sort_by(|a, b| a.0.cmp(&b.0));

    let mut child_rendered = rendered.to_vec();
    for (p, u) in &to_emit {
        child_rendered.retain(|(rp, _)| rp != p);
        child_rendered.push((p.clone(), u.clone()));
    }

    out.push('<');
    out.push_str(&qname);
    for (p, u) in &to_emit {
        if p.is_empty() {
            out.push_str(" xmlns=\"");
        } else {
            out.push_str(" xmlns:");
            out.push_str(p);
            out.push_str("=\"");
        }
        out.push_str(&escape_attr(u));
        out.push('"');
    }

    // 属性排序：(namespace-uri 或 "", local-name)；xmlns 声明不在 attributes() 里。
    let mut attrs: Vec<(String, String, String, String)> = Vec::new();
    for a in node.attributes() {
        let au = a.namespace().unwrap_or("");
        let ap = a
            .namespace()
            .and_then(|u| node.lookup_prefix(u))
            .unwrap_or("");
        attrs.push((
            au.to_string(),
            a.name().to_string(),
            ap.to_string(),
            a.value().to_string(),
        ));
    }
    attrs.sort_by(|x, y| (x.0.as_str(), x.1.as_str()).cmp(&(y.0.as_str(), y.1.as_str())));
    for (_, alocal, ap, value) in &attrs {
        out.push(' ');
        if !ap.is_empty() {
            out.push_str(ap);
            out.push(':');
        }
        out.push_str(alocal);
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }
    out.push('>');

    for child in node.children() {
        if Some(child.id()) == omit {
            continue;
        }
        if child.is_element() {
            // inclusive 仅 apex 生效，子层传空。
            c14n_element(out, child, omit, &[], &child_rendered);
        } else if child.is_text()
            && let Some(t) = child.text()
        {
            out.push_str(&escape_text(t));
        }
    }

    out.push_str("</");
    out.push_str(&qname);
    out.push('>');
}

/// c14n 文本节点转义：`&`/`<`/`>`/`\r`。
fn escape_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '\r' => o.push_str("&#xD;"),
            _ => o.push(c),
        }
    }
    o
}

/// c14n 属性值转义：`&`/`<`/`"`/`\t`/`\n`/`\r`。
fn escape_attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '"' => o.push_str("&quot;"),
            '\t' => o.push_str("&#x9;"),
            '\n' => o.push_str("&#xA;"),
            '\r' => o.push_str("&#xD;"),
            _ => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::{
        rand::SystemRandom,
        rsa::KeySize,
        signature::{KeyPair, RSA_PKCS1_SHA256, RsaKeyPair},
    };

    use super::*;

    const TEST_RSA_PRIVATE_KEY_DER_B64: &str = "MIIEowIBAAKCAQEAloc1makauKWi+BfoS/5cBi8dTcRDTk0T+rAMoxQ6POHLMCcWHeqI7LmoiE9Fhij0VNMHJ0KTFRnIETrxuhMDXOmetB5FKaW1oIRaracbYbv2igjWMMoPQHeZoowBQ2gO6up7PuiTaP3xU9Gbm41wI3/qNjEXB/bHBRfYpZHpHg5Pwuu+XvytWodA6qARDnG+X060SSwDCg1Rjl1bpxKnGOECwYjGk4xxt0sCtvHGMDFf0DYZmiKWzrlACRR8lAZtnMtRZlDLAJrhkxZQ69DvE/p3co2AWB1qY2NFcrI+MyyJ0IYftria+x3AkFAILHc5lYmOFPZtSwv6/WmF4l2BUQIDAQABAoIBAD00NrgGyQWg23WSivIzNwmQqJKRJWi18FSytkCsnc1ZZXtruRNq5EeUaZEa4kLdbhL7RuJQsGsY838F/3lP98MUfmHwGOEJLqNYjWIkGv8PJPwVAboOvkrRFlfAI1db/QzJUFlQyXccIb6Z8HI7/QeCICHurdWgImXQv1b6z9VXDpUc1sAri0+BFCU+BkJdn3MbSXovUBqiz5icKod1dbufUik3Yfo9YZDGHGEOHShUq+AKppe8NrAslBMKtQY0NOPE/c9lNm01/riSgQcg3hnn50kHl+jwRrTc3CUk3o7Ey5J2jld4gRseAQehfe/dgXXwJfQ/DmYuqzRmgFdhxf0CgYEAzHCr2Hw1f7v9wMR4oGzOW1oJsPNhhltpZmZ2lmS0sEbTTBjNk2vIMUX/ytLBuhdDF4hYrHmkzIGDkI/x0WEaslL5c370mGbCE7sEheD6UTwwMJs3h5ijEmawKZ9ZY9PVYFNJ8YfaETwQgce2U81qzYVebvSu+rcjcVexRJJzvacCgYEAvH3N2A+o1Zu6ZPeu1mBUlOc2J6U2+iHKqWajU9T9kJc6lx0nf6aMQP+PA5kwpoR0dmg3cEY62BCYlob4wxLIbLZVPPZO/VhGRJKR1xfY64mgeXYm1wMq6m0z6SrZKJPb6NS+qmvxaROEJCCgB7qA8/N8Rg9QGOwSaUfI1G+p2EcCgYB4T7ZR6IbzbQagcv8qKd4nFI2vfQtfrlwQzyvqxckwE/41QkN5Bm0B0lf+XJl1kksBhlPo7I13bKCoao280pCLcRksRwJazd5ZDi5TO1sUg384m5/KRKFzKstxMz2/6eIgleNmKLTEf7yXI5jBKJo56MryMTzofu50vU6tNCK48QKBgHFqmVNqiMKPQ6bBShiAOiSmwvUz+lKjxpgLxDcLL8+yz3Rh/IRYqIfrvhgCMz3e8VzV9JXADGQ6CDZ63HA0exi+1acq5fiXByD3uH1eQg3n8AFl0JULuOT59IRWXfiGj4oXiOpurQH21koOv3wnArTHS320dROp6KIkqXj5/469AoGBAK2T22cmxe6o9N4sCBBoZlYJg7XX34qCxAciJqnAUR1DEZmoZfPrrNix9xu+uW73r0eJCFsu3aGpw2+blvB4deTrx0UtlRXhk2qYn60eajFwpnRqSbiWgNCzehrbyO59t2rCk7BVHLKbGCehU4rMj+uy9MPFz+otllALp4/U5vjn";

    fn test_keypair() -> (RsaKeyPair, String) {
        use base64::Engine as _;
        let der = base64::engine::general_purpose::STANDARD
            .decode(TEST_RSA_PRIVATE_KEY_DER_B64)
            .expect("test private key der b64");
        let key_pair = RsaKeyPair::from_der(&der).expect("test rsa keypair");
        let public_b64 =
            base64::engine::general_purpose::STANDARD.encode(key_pair.public_key().as_ref());
        let pem =
            format!("-----BEGIN RSA PUBLIC KEY-----\n{public_b64}\n-----END RSA PUBLIC KEY-----");
        (key_pair, pem)
    }

    fn sign(sk: &RsaKeyPair, msg: &[u8]) -> String {
        let mut sig = vec![0; sk.public_modulus_len()];
        sk.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), msg, &mut sig)
            .expect("rsa sign");
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(sig)
    }

    /// 自签 Response：固定格式，方便精确控制 byte range。
    fn build_signed_response(sk: &RsaKeyPair, tamper_assertion: bool) -> Vec<u8> {
        // 1. 先把 Signature 留个占位，算 SignedInfo + DigestValue
        let mut assertion_inner =
            "<saml:Subject><saml:NameID>alice@example.com</saml:NameID></saml:Subject>".to_string();
        if tamper_assertion {
            assertion_inner =
                "<saml:Subject><saml:NameID>attacker@evil.com</saml:NameID></saml:Subject>"
                    .to_string();
        }
        // 假设要先算"剪掉 Signature 后的 assertion"的 SHA256
        let assertion_open = "<saml:Assertion>";
        let assertion_close = "</saml:Assertion>";
        // 不带 Signature 的 assertion bytes（用于算 DigestValue）
        let assertion_no_sig = format!("{assertion_open}{assertion_inner}{assertion_close}");
        let digest = sha2::Sha256::digest(assertion_no_sig.as_bytes());
        use base64::Engine as _;
        let digest_b64 = base64::engine::general_purpose::STANDARD.encode(digest);
        let signed_info = format!(
            r#"<ds:SignedInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><ds:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><ds:Reference URI=""><ds:Transforms><ds:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/></ds:Transforms><ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><ds:DigestValue>{digest_b64}</ds:DigestValue></ds:Reference></ds:SignedInfo>"#
        );
        let sig_b64 = sign(sk, signed_info.as_bytes());
        let signature_xml = format!(
            r#"<ds:Signature xmlns:ds="http://www.w3.org/2000/09/xmldsig#">{signed_info}<ds:SignatureValue>{sig_b64}</ds:SignatureValue></ds:Signature>"#
        );
        // assertion 把 Signature 插入到第一个子元素位置
        let assertion_xml =
            format!("{assertion_open}{signature_xml}{assertion_inner}{assertion_close}");
        format!(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">{assertion_xml}</samlp:Response>"#
        )
        .into_bytes()
    }

    #[test]
    fn happy_path_verifies() {
        let (sk, pem) = test_keypair();
        let xml = build_signed_response(&sk, false);
        verify_assertion_signature(&xml, &pem).expect("legit signature should verify");
    }

    #[test]
    fn tampered_assertion_fails() {
        let (sk, pem) = test_keypair();
        // 先生成合法 response，然后篡改 NameID
        let mut xml = build_signed_response(&sk, false);
        let s = std::str::from_utf8(&xml).unwrap().to_string();
        let tampered = s.replace("alice@example.com", "attacker@evil.com");
        xml = tampered.into_bytes();
        let err = verify_assertion_signature(&xml, &pem)
            .expect_err("tampered assertion must fail digest");
        let msg = format!("{err}");
        assert!(msg.contains("digest mismatch") || msg.contains("RSA verify"));
    }

    #[test]
    fn wrong_key_fails() {
        let (sk, _pem) = test_keypair();
        let other = RsaKeyPair::generate(KeySize::Rsa2048).expect("other rsa keypair");
        use base64::Engine as _;
        let other_b64 =
            base64::engine::general_purpose::STANDARD.encode(other.public_key().as_ref());
        let other_pem =
            format!("-----BEGIN RSA PUBLIC KEY-----\n{other_b64}\n-----END RSA PUBLIC KEY-----");
        let xml = build_signed_response(&sk, false);
        let err = verify_assertion_signature(&xml, &other_pem).expect_err("wrong key must fail");
        let msg = format!("{err}");
        assert!(msg.contains("RSA verify") || msg.contains("digest"));
    }

    // ===== exclusive C14N 路径 =====

    const NS: &str = r#"xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:ds="http://www.w3.org/2000/09/xmldsig#""#;

    fn c14n_of(doc_xml: &str, local: &str) -> String {
        let doc = Document::parse(doc_xml).unwrap();
        let node = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == local)
            .unwrap();
        canonicalize_exclusive(node, None, &[])
    }

    /// 构造命名空间声明在 **root**、Assertion 靠继承使用的签名 Response —— 正是 raw-bytes
    /// 路径处理不了（assertion 子串不含 xmlns:saml）而 exclusive C14N 能处理的情形。
    fn build_c14n_signed_response(sk: &RsaKeyPair, name_id: &str) -> Vec<u8> {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD;
        let subject = format!("<saml:Subject><saml:NameID>{name_id}</saml:NameID></saml:Subject>");
        // (a) DigestValue = SHA256(c14n(Assertion 无 Signature))。
        let doc_a = format!(
            "<samlp:Response {NS}><saml:Assertion ID=\"a1\">{subject}</saml:Assertion></samlp:Response>"
        );
        let digest_b64 = b64.encode(sha2::Sha256::digest(
            c14n_of(&doc_a, "Assertion").as_bytes(),
        ));
        // (b) SignatureValue = RSA(c14n(SignedInfo))，SignedInfo 处于与最终文档相同的命名空间上下文。
        let signed_info = format!(
            r#"<ds:SignedInfo><ds:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><ds:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><ds:Reference URI=""><ds:Transforms><ds:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/></ds:Transforms><ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><ds:DigestValue>{digest_b64}</ds:DigestValue></ds:Reference></ds:SignedInfo>"#
        );
        let doc_b = format!(
            "<samlp:Response {NS}><saml:Assertion ID=\"a1\"><ds:Signature>{signed_info}</ds:Signature>{subject}</saml:Assertion></samlp:Response>"
        );
        let sig_b64 = sign(sk, c14n_of(&doc_b, "SignedInfo").as_bytes());
        // (c) 最终文档。
        format!(
            "<samlp:Response {NS}><saml:Assertion ID=\"a1\"><ds:Signature>{signed_info}<ds:SignatureValue>{sig_b64}</ds:SignatureValue></ds:Signature>{subject}</saml:Assertion></samlp:Response>"
        )
        .into_bytes()
    }

    #[test]
    fn c14n_path_verifies_inherited_namespaces() {
        let (sk, pem) = test_keypair();
        let xml = build_c14n_signed_response(&sk, "alice@example.com");
        // raw-bytes 路径无法处理命名空间继承 → 失败；exclusive C14N 回退 → 成功。
        assert!(
            verify_raw_bytes(&xml, &pem).is_err(),
            "raw path can't handle namespace inheritance"
        );
        verify_assertion_signature(&xml, &pem).expect("c14n fallback verifies");
    }

    #[test]
    fn c14n_path_rejects_tampered() {
        let (sk, pem) = test_keypair();
        let xml = build_c14n_signed_response(&sk, "alice@example.com");
        let tampered = String::from_utf8(xml)
            .unwrap()
            .replace("alice@example.com", "attacker@evil.com");
        verify_assertion_signature(tampered.as_bytes(), &pem)
            .expect_err("tampered must fail both raw and c14n paths");
    }
}
