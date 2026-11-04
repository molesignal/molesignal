// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Meet：服务账号代组织里的 `organizer` 建会议。服务账号签一个 JWT（RS256，`sub` 是组织者），在
//! `https://oauth2.googleapis.com/token` 换令牌，再 `POST https://meet.googleapis.com/v2/spaces` 建一个
//! 会议空间，返回 `meetingUri`。Google Workspace 里要给服务账号开全网域委派，授权下面的范围。

use std::io::Cursor;

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::pki_types::PrivateKeyDer;
use serde_json::json;

use super::{client, send, unix_seconds};
use crate::{
    domain::alerting::meeting::{GoogleMeetSettings, ServiceAccountKey},
    shared::{Error, Result},
};

const SCOPE: &str = "https://www.googleapis.com/auth/meetings.space.created";
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const SPACES: &str = "https://meet.googleapis.com/v2/spaces";
const JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

pub(super) async fn book(settings: &GoogleMeetSettings, credentials: &str) -> Result<String> {
    let key = ServiceAccountKey::parse(credentials)?;
    let assertion = assertion(&key, &settings.organizer, unix_seconds()?)?;

    let issued = send(
        client(TOKEN_URI).await?.post(TOKEN_URI).form(&[
            ("grant_type", JWT_BEARER),
            ("assertion", assertion.as_str()),
        ]),
        "Google",
    )
    .await?;

    let token = issued["access_token"]
        .as_str()
        .ok_or_else(|| Error::unavailable("Google did not issue a token"))?;

    let space = send(
        client(SPACES)
            .await?
            .post(SPACES)
            .bearer_auth(token)
            .json(&json!({})),
        "Google Meet",
    )
    .await?;

    space["meetingUri"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Google Meet did not return the meeting link"))
}

/// 换令牌用的断言：服务账号签的 JWT，代 `subject` 办事，一小时有效。
fn assertion(key: &ServiceAccountKey, subject: &str, now: i64) -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());
    let claims = URL_SAFE_NO_PAD.encode(
        json!({
            "iss": key.client_email.trim(),
            "sub": subject,
            "scope": SCOPE,
            "aud": TOKEN_URI,
            "iat": now,
            "exp": now + 3_600,
        })
        .to_string(),
    );
    let signed = format!("{header}.{claims}");

    let pair = signing_key(&key.private_key)?;
    let mut signature = vec![0; pair.public_modulus_len()];

    pair.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| Error::internal("the Google token request could not be signed"))?;

    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn signing_key(pem: &str) -> Result<RsaKeyPair> {
    let invalid = || Error::invalid("the Google service account key is not an RSA private key");
    let key = rustls_pemfile::private_key(&mut Cursor::new(pem.as_bytes()))
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;

    match key {
        PrivateKeyDer::Pkcs8(der) => RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()),
        PrivateKeyDer::Pkcs1(der) => RsaKeyPair::from_der(der.secret_pkcs1_der()),
        _ => return Err(invalid()),
    }
    .map_err(|_| invalid())
}
