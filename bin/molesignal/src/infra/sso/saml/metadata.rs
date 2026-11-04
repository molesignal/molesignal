// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SAML IdP 元数据：从 `EntityDescriptor` 取实体 ID、HTTP-Redirect 单点登录地址和
//! 签名证书，给管理端填表。登录流程用 HTTP-Redirect binding 发 AuthnRequest，所以
//! 只认这种 binding 的 `SingleSignOnService`。

use std::time::Duration;

use reqwest::Client;
use roxmltree::{Document, Node};
use serde::Serialize;
use url::Url;

use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

const METADATA_NS: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
const DSIG_NS: &str = "http://www.w3.org/2000/09/xmldsig#";
const REDIRECT_BINDING: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect";

/// 元数据文档的大小上限。
pub const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SamlIdpMetadata {
    pub idp_entity_id: String,
    pub idp_sso_url: String,
    /// PEM 格式；元数据列了几张签名证书时取第一张。
    pub idp_x509_cert: String,
}

pub async fn fetch(url: &str) -> Result<String> {
    let url = Url::parse(url.trim()).map_err(|e| Error::invalid(format!("metadata URL: {e}")))?;

    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::invalid("the metadata URL must be http(s)"));
    }

    let http = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::internal(format!("metadata http client: {e}")))?;

    let response = http_trace::send(&http, http.get(url.clone()), HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::invalid(format!("could not reach {url}: {e}")))?;

    if !response.status().is_success() {
        return Err(Error::invalid(format!(
            "{url} returned {}",
            response.status()
        )));
    }

    let body = response
        .bytes()
        .await
        .map_err(|e| Error::invalid(format!("could not read {url}: {e}")))?;

    if body.len() > MAX_METADATA_BYTES {
        return Err(Error::invalid("the metadata document is larger than 2 MiB"));
    }

    String::from_utf8(body.to_vec()).map_err(|_| Error::invalid("the metadata is not UTF-8 text"))
}

pub fn parse(xml: &str) -> Result<SamlIdpMetadata> {
    if xml.len() > MAX_METADATA_BYTES {
        return Err(Error::invalid("the metadata document is larger than 2 MiB"));
    }

    let document =
        Document::parse(xml).map_err(|e| Error::invalid(format!("metadata XML: {e}")))?;

    let idp = document
        .descendants()
        .find(|node| is(node, METADATA_NS, "IDPSSODescriptor"))
        .ok_or_else(|| Error::invalid("the metadata has no IDPSSODescriptor"))?;

    let idp_entity_id = idp
        .ancestors()
        .find(|node| is(node, METADATA_NS, "EntityDescriptor"))
        .and_then(|entity| entity.attribute("entityID"))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::invalid("the metadata has no entityID"))?
        .to_owned();

    let idp_sso_url = idp
        .children()
        .filter(|node| is(node, METADATA_NS, "SingleSignOnService"))
        .find(|node| node.attribute("Binding") == Some(REDIRECT_BINDING))
        .and_then(|node| node.attribute("Location"))
        .map(str::trim)
        .filter(|location| !location.is_empty())
        .ok_or_else(|| Error::invalid("the metadata has no HTTP-Redirect SingleSignOnService"))?
        .to_owned();

    let mut keys = idp
        .children()
        .filter(|node| is(node, METADATA_NS, "KeyDescriptor"));

    let signing_key = keys
        .find(|node| matches!(node.attribute("use"), None | Some("signing")))
        .ok_or_else(|| Error::invalid("the metadata has no signing KeyDescriptor"))?;

    let certificate = signing_key
        .descendants()
        .find(|node| is(node, DSIG_NS, "X509Certificate"))
        .and_then(|node| node.text())
        .map(|text| text.split_whitespace().collect::<String>())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| Error::invalid("the signing KeyDescriptor has no X509Certificate"))?;

    Ok(SamlIdpMetadata {
        idp_entity_id,
        idp_sso_url,
        idp_x509_cert: pem(&certificate),
    })
}

fn is(node: &Node<'_, '_>, namespace: &str, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node.tag_name().namespace() == Some(namespace)
}

fn pem(base64: &str) -> String {
    let lines = base64
        .as_bytes()
        .chunks(64)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>()
        .join("\n");

    format!("-----BEGIN CERTIFICATE-----\n{lines}\n-----END CERTIFICATE-----")
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = r#"<?xml version="1.0"?>
<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="https://sts.windows.net/tenant-1/">
  <md:IDPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <md:KeyDescriptor use="encryption">
      <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data><ds:X509Certificate>ENCRYPTION</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
    </md:KeyDescriptor>
    <md:KeyDescriptor use="signing">
      <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data><ds:X509Certificate>
        MIIC8DCCAdigAwIBAgIQ
        AAAA
      </ds:X509Certificate></ds:X509Data></ds:KeyInfo>
    </md:KeyDescriptor>
    <md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://login.microsoftonline.com/tenant-1/saml2/post"/>
    <md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://login.microsoftonline.com/tenant-1/saml2"/>
  </md:IDPSSODescriptor>
</md:EntityDescriptor>"#;

    #[test]
    fn reads_entity_redirect_location_and_signing_certificate() {
        let metadata = parse(METADATA).expect("metadata");
        assert_eq!(metadata.idp_entity_id, "https://sts.windows.net/tenant-1/");
        assert_eq!(
            metadata.idp_sso_url,
            "https://login.microsoftonline.com/tenant-1/saml2"
        );
        assert_eq!(
            metadata.idp_x509_cert,
            "-----BEGIN CERTIFICATE-----\nMIIC8DCCAdigAwIBAgIQAAAA\n-----END CERTIFICATE-----"
        );
    }

    #[test]
    fn sp_metadata_is_rejected() {
        let sp = r#"<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata" entityID="sp"><SPSSODescriptor/></EntityDescriptor>"#;
        assert!(parse(sp).is_err());
    }

    #[test]
    fn long_certificates_are_wrapped_at_64_columns() {
        let wrapped = pem(&"A".repeat(130));
        let lines = wrapped.lines().collect::<Vec<_>>();
        assert_eq!(lines[1].len(), 64);
        assert_eq!(lines[3].len(), 2);
    }
}
