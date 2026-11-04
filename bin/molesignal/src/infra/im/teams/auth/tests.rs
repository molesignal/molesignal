// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::{
    rand::SystemRandom,
    rsa::KeySize,
    signature::{KeyPair, RSA_PKCS1_SHA256, RsaKeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::*;

const APP_ID: &str = "00000000-0000-0000-0000-00000000abcd";
const SERVICE: &str = "https://smba.trafficmanager.net/teams/";

/// 本机生成的签名密钥：私钥签令牌，公钥放进 JWKS。
struct Signer {
    key: RsaKeyPair,
}

/// PKCS#1 的 RSAPublicKey 里依次是 n 与 e 两个整数。
fn integers(der: &[u8]) -> Vec<Vec<u8>> {
    fn length(der: &[u8], at: &mut usize) -> usize {
        let first = der[*at];
        *at += 1;

        if first < 0x80 {
            return usize::from(first);
        }

        let mut length = 0;

        for _ in 0..(first & 0x7f) {
            length = (length << 8) | usize::from(der[*at]);
            *at += 1;
        }

        length
    }

    let mut at = 1;
    length(der, &mut at);
    let mut values = Vec::new();

    while at < der.len() {
        assert_eq!(der[at], 0x02);
        at += 1;
        let size = length(der, &mut at);
        let value = &der[at..at + size];
        values.push(
            value
                .iter()
                .skip_while(|byte| **byte == 0)
                .copied()
                .collect(),
        );
        at += size;
    }

    values
}

impl Signer {
    fn new() -> Self {
        Self {
            key: RsaKeyPair::generate(KeySize::Rsa2048).unwrap(),
        }
    }

    fn jwk(&self, kid: &str, endorsements: &[&str]) -> Value {
        let parts = integers(self.key.public_key().as_ref());

        json!({
            "kty": "RSA",
            "use": "sig",
            "kid": kid,
            "n": URL_SAFE_NO_PAD.encode(&parts[0]),
            "e": URL_SAFE_NO_PAD.encode(&parts[1]),
            "endorsements": endorsements,
        })
    }

    fn sign(&self, kid: &str, alg: &str, claims: &Value) -> String {
        let header =
            URL_SAFE_NO_PAD.encode(json!({ "alg": alg, "typ": "JWT", "kid": kid }).to_string());
        let body = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signed = format!("{header}.{body}");
        let mut signature = vec![0; self.key.public_modulus_len()];

        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed.as_bytes(),
                &mut signature,
            )
            .unwrap();

        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature))
    }
}

fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

fn claims() -> Value {
    json!({
        "iss": "https://api.botframework.com",
        "aud": APP_ID,
        "exp": now() + 600,
        "nbf": now() - 10,
        "serviceUrl": SERVICE,
    })
}

fn activity(channel: &str, service: &str) -> Value {
    json!({ "type": "message", "channelId": channel, "serviceUrl": service })
}

/// 本机的 Bot Framework：OpenID 元数据指向它的 JWKS。wiremock 会复用服务器，地址里加一段
/// 只属于这次的路径，免得读到别的测试缓存的密钥。
async fn bot_framework(keys: Vec<Value>) -> (MockServer, Trust) {
    let server = MockServer::start().await;
    let root = format!("/{}", crate::shared::ids::Id::new().0);

    Mock::given(method("GET"))
        .and(path(format!("{root}/v1/.well-known/openidconfiguration")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": "https://api.botframework.com",
            "jwks_uri": format!("{}{root}/v1/.well-known/keys", server.uri()),
            "id_token_signing_alg_values_supported": ["RS256"],
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{root}/v1/.well-known/keys")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": keys })))
        .mount(&server)
        .await;

    let trust = Trust {
        metadata_url: format!("{}{root}/v1/.well-known/openidconfiguration", server.uri()),
        issuer: "https://api.botframework.com".into(),
    };

    (server, trust)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
async fn a_token_signed_by_bot_framework_for_this_bot_is_accepted() {
    let signer = Signer::new();
    let (_server, trust) = bot_framework(vec![signer.jwk("k1", &["msteams"])]).await;
    let token = signer.sign("k1", "RS256", &claims());

    authenticate(
        &trust,
        Some(&bearer(&token)),
        APP_ID,
        &activity("msteams", SERVICE),
    )
    .await
    .unwrap();

    // 末尾的斜杠不算不同。
    authenticate(
        &trust,
        Some(&bearer(&token)),
        APP_ID,
        &activity("msteams", "https://smba.trafficmanager.net/teams"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_token_for_someone_else_or_out_of_date_is_unauthorized() {
    let signer = Signer::new();
    let (_server, trust) = bot_framework(vec![signer.jwk("k1", &["msteams"])]).await;
    let teams = activity("msteams", SERVICE);

    let mut other_bot = claims();
    other_bot["aud"] = json!("someone-else");
    let mut other_issuer = claims();
    other_issuer["iss"] = json!("https://sts.windows.net/x/");
    let mut expired = claims();
    expired["exp"] = json!(now() - 600);
    let mut not_yet = claims();
    not_yet["nbf"] = json!(now() + 600);

    for claims in [other_bot, other_issuer, expired, not_yet] {
        let token = signer.sign("k1", "RS256", &claims);
        let error = authenticate(&trust, Some(&bearer(&token)), APP_ID, &teams)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unauthorized(_)), "{error}");
    }

    // 别人的私钥签的、不认识的密钥、HS256、没有令牌。
    let stranger = Signer::new().sign("k1", "RS256", &claims());
    let unknown = signer.sign("k9", "RS256", &claims());
    let hs256 = signer.sign("k1", "HS256", &claims());

    for authorization in [
        Some(bearer(&stranger)),
        Some(bearer(&unknown)),
        Some(bearer(&hs256)),
        Some("Basic abc".to_string()),
        None,
    ] {
        let error = authenticate(&trust, authorization.as_deref(), APP_ID, &teams)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unauthorized(_)), "{error}");
    }
}

#[tokio::test]
async fn a_key_must_endorse_the_channel_and_the_service_urls_must_match() {
    let signer = Signer::new();
    let (_server, trust) = bot_framework(vec![signer.jwk("k1", &["msteams"])]).await;
    let token = signer.sign("k1", "RS256", &claims());

    let webchat = authenticate(
        &trust,
        Some(&bearer(&token)),
        APP_ID,
        &activity("webchat", SERVICE),
    )
    .await
    .unwrap_err();
    assert!(matches!(webchat, Error::Forbidden(_)), "{webchat}");

    let elsewhere = authenticate(
        &trust,
        Some(&bearer(&token)),
        APP_ID,
        &activity("msteams", "https://evil.example/teams/"),
    )
    .await
    .unwrap_err();
    assert!(matches!(elsewhere, Error::Forbidden(_)), "{elsewhere}");
}
