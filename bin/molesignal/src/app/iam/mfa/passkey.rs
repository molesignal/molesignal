// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Passkey（WebAuthn）：依赖方、账号在验证器里的用户句柄、注册参数和验证器型号。
//!
//! 依赖方（RP）取 `[http].external_url`：RP ID 是它的主机名，origin 就是它。iCloud 钥匙串和
//! Google 密码管理器要求 RP ID 是域名，浏览器要求安全上下文，所以没配、主机名是 IP、或者不是
//! HTTPS（localhost 除外）时 Passkey 不可用。localhost 上不比端口：开发时页面和接口各占一个端口。
//!
//! 注册在 webauthn-rs 的 `start_passkey_registration` 上改两处：要求可发现凭据，不带 credProtect
//! 扩展。安卓上不这样就存不进 Google 密码管理器（库的注释也这样说），免用户名登录也靠可发现凭据。
//! 其余照库的默认：attestation 只要 none，不限验证器类型，要求用户验证，同步型 Passkey 的备份
//! 标志和恒为 0 的签名计数都照常接受。凭据仍由库校验。

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, PasskeyRegistration, RegisterPublicKeyCredential, Url,
    Uuid, Webauthn, WebauthnBuilder,
};
use webauthn_rs_proto::ResidentKeyRequirement;

use crate::{
    domain::iam::User,
    shared::{Error, Result, ids::Id},
};

/// Passkey 为什么不可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// 没配 `[http].external_url`。
    NotConfigured,
    /// 对外地址的主机名是 IP。
    IpAddress,
    /// 对外地址不是 HTTPS，也不是 localhost。
    NotHttps,
}

/// 按对外地址建的依赖方。
pub fn relying_party(external_url: &str) -> std::result::Result<Webauthn, Unavailable> {
    let url = Url::parse(external_url.trim()).map_err(|_| Unavailable::NotConfigured)?;

    let host = match url.host() {
        Some(url::Host::Domain(host)) => host.to_ascii_lowercase(),
        Some(_) => return Err(Unavailable::IpAddress),
        None => return Err(Unavailable::NotConfigured),
    };

    let local = host == "localhost" || host.ends_with(".localhost");

    if url.scheme() != "https" && !local {
        return Err(Unavailable::NotHttps);
    }

    let origin =
        Url::parse(&url.origin().ascii_serialization()).map_err(|_| Unavailable::NotConfigured)?;

    WebauthnBuilder::new(&host, &origin)
        .and_then(|builder| builder.rp_name("MoleSignal").allow_any_port(local).build())
        .map_err(|_| Unavailable::NotConfigured)
}

/// 账号在验证器里的用户句柄：账号 ID 摘要的前 16 字节。免用户名登录时验证器把它交回来，用来
/// 核对这个凭据确实属于这个账号。
pub fn user_handle(user_id: &Id) -> Uuid {
    let digest = Sha256::digest(format!("molesignal.passkey.user:{}", user_id.0).as_bytes());
    let mut handle = [0u8; 16];
    handle.copy_from_slice(&digest[..16]);

    Uuid::from_bytes(handle)
}

/// 给浏览器的注册参数和留在服务端的中间状态；`existing` 是账号已有的凭据，不再重复添加。
pub fn start_registration(
    webauthn: &Webauthn,
    user: &User,
    existing: Vec<CredentialID>,
) -> Result<(CreationChallengeResponse, PasskeyRegistration)> {
    let display_name = if user.display_name.trim().is_empty() {
        &user.email
    } else {
        &user.display_name
    };

    let (mut options, registration) = webauthn
        .start_passkey_registration(
            user_handle(&user.id),
            &user.email,
            display_name,
            Some(existing),
        )
        .map_err(|error| Error::internal(format!("passkey registration options: {error}")))?;

    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }

    if let Some(extensions) = options.public_key.extensions.as_mut() {
        extensions.cred_protect = None;
    }

    Ok((options, registration))
}

/// 注册应答里的验证器型号（AAGUID，32 位十六进制），只用来显示 Passkey 存在哪里。浏览器隐去了
/// （全 0）或读不出来时为空。
pub fn aaguid(credential: &RegisterPublicKeyCredential) -> Option<String> {
    let object: ciborium::Value =
        ciborium::from_reader(credential.response.attestation_object.as_slice()).ok()?;

    let auth_data = object
        .as_map()?
        .iter()
        .find(|(key, _)| key.as_text() == Some("authData"))?
        .1
        .as_bytes()?;

    // 认证器数据：RP ID 的摘要 32 字节、标志 1 字节、签名计数 4 字节；带着凭据（AT 标志）时
    // 接着是 16 字节的 AAGUID。
    let aaguid = auth_data
        .get(37..53)
        .filter(|_| auth_data[32] & 0x40 != 0)?;

    aaguid
        .iter()
        .any(|byte| *byte != 0)
        .then(|| hex::encode(aaguid))
}

/// webauthn-rs 的中间状态，存进等待验证的请求。
pub fn stored_state<T: Serialize>(state: &T) -> Result<serde_json::Value> {
    serde_json::to_value(state)
        .map_err(|error| Error::internal(format!("passkey state encode: {error}")))
}

/// 从等待验证的请求里取回中间状态。
pub fn read_state<T: DeserializeOwned>(state: Option<serde_json::Value>) -> Result<T> {
    let state = state.ok_or_else(|| Error::internal("passkey request without its state"))?;

    serde_json::from_value(state)
        .map_err(|error| Error::internal(format!("passkey state decode: {error}")))
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::{
        rand::SystemRandom,
        signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use ciborium::Value;
    use serde_json::json;
    use webauthn_rs::prelude::{
        DiscoverableAuthentication, DiscoverableKey, Passkey, PasskeyAuthentication,
        PublicKeyCredential, RequestChallengeResponse,
    };

    use super::*;
    use crate::{domain::iam::UserStatus, shared::time::TimestampMicros};

    const ORIGIN: &str = "https://ops.example.com";
    /// iCloud 钥匙串的 AAGUID。
    const ICLOUD_KEYCHAIN: &str = "fbfc3007154e4ecc8c0b6e020557d7bd";
    /// 用户在场、已验证、可备份、已备份。
    const SYNCED: u8 = 0x01 | 0x04 | 0x08 | 0x10;
    /// 带着新凭据。
    const ATTESTED: u8 = 0x40;

    fn user() -> User {
        User {
            id: Id::from_string("2Yb8w0D3k1u9sQZpWm4cXrT7aLf"),
            email: "alice@example.com".into(),
            display_name: "Alice".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: String::new(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(0),
            sessions_valid_after: TimestampMicros(0),
        }
    }

    fn b64(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn cbor(value: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        ciborium::into_writer(value, &mut out).unwrap();
        out
    }

    fn client_data(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "type": kind,
            "challenge": challenge,
            "origin": origin,
            "crossOrigin": false,
        }))
        .unwrap()
    }

    fn authenticator_data(rp_id: &str, flags: u8, attested: &[u8]) -> Vec<u8> {
        let mut data = Sha256::digest(rp_id.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(attested);
        data
    }

    /// 软件验证器：P-256 密钥，像 iCloud 钥匙串一样报告备份标志，签名计数恒为 0。
    struct SoftPasskey {
        key: EcdsaKeyPair,
        credential_id: Vec<u8>,
        user_handle: Vec<u8>,
        aaguid: Vec<u8>,
    }

    impl SoftPasskey {
        fn new(user_handle: Uuid) -> Self {
            let key = EcdsaKeyPair::generate(&ECDSA_P256_SHA256_ASN1_SIGNING).unwrap();
            let point: &[u8] = key.public_key().as_ref();
            let credential_id = Sha256::digest(point)[..16].to_vec();

            Self {
                key,
                credential_id,
                user_handle: user_handle.as_bytes().to_vec(),
                aaguid: hex::decode(ICLOUD_KEYCHAIN).unwrap(),
            }
        }

        fn cose_key(&self) -> Vec<u8> {
            let point: &[u8] = self.key.public_key().as_ref();

            cbor(&Value::Map(vec![
                (Value::Integer(1.into()), Value::Integer(2.into())),
                (Value::Integer(3.into()), Value::Integer((-7).into())),
                (Value::Integer((-1).into()), Value::Integer(1.into())),
                (
                    Value::Integer((-2).into()),
                    Value::Bytes(point[1..33].to_vec()),
                ),
                (
                    Value::Integer((-3).into()),
                    Value::Bytes(point[33..65].to_vec()),
                ),
            ]))
        }

        fn create(
            &self,
            options: &CreationChallengeResponse,
            origin: &str,
        ) -> RegisterPublicKeyCredential {
            let challenge = serde_json::to_value(options).unwrap()["publicKey"]["challenge"]
                .as_str()
                .unwrap()
                .to_owned();

            let mut attested = self.aaguid.clone();
            attested.extend_from_slice(&(self.credential_id.len() as u16).to_be_bytes());
            attested.extend_from_slice(&self.credential_id);
            attested.extend_from_slice(&self.cose_key());

            let data = authenticator_data(&options.public_key.rp.id, SYNCED | ATTESTED, &attested);

            let attestation = cbor(&Value::Map(vec![
                (Value::Text("fmt".into()), Value::Text("none".into())),
                (Value::Text("attStmt".into()), Value::Map(Vec::new())),
                (Value::Text("authData".into()), Value::Bytes(data)),
            ]));

            serde_json::from_value(json!({
                "id": b64(&self.credential_id),
                "rawId": b64(&self.credential_id),
                "type": "public-key",
                "response": {
                    "attestationObject": b64(&attestation),
                    "clientDataJSON": b64(&client_data("webauthn.create", &challenge, origin)),
                    "transports": ["internal", "hybrid"],
                },
                "extensions": {},
            }))
            .unwrap()
        }

        fn get(&self, options: &RequestChallengeResponse, origin: &str) -> PublicKeyCredential {
            let challenge = serde_json::to_value(options).unwrap()["publicKey"]["challenge"]
                .as_str()
                .unwrap()
                .to_owned();

            let data = authenticator_data(&options.public_key.rp_id, SYNCED, &[]);
            let client = client_data("webauthn.get", &challenge, origin);
            let mut signed = data.clone();
            signed.extend_from_slice(&Sha256::digest(&client));
            let signature = self.key.sign(&SystemRandom::new(), &signed).unwrap();

            serde_json::from_value(json!({
                "id": b64(&self.credential_id),
                "rawId": b64(&self.credential_id),
                "type": "public-key",
                "response": {
                    "authenticatorData": b64(&data),
                    "clientDataJSON": b64(&client),
                    "signature": b64(signature.as_ref()),
                    "userHandle": b64(&self.user_handle),
                },
                "extensions": {},
            }))
            .unwrap()
        }
    }

    /// 中间状态像存进库里那样走一遍 JSON。
    fn through_storage<T: Serialize + DeserializeOwned>(state: &T) -> T {
        read_state(Some(stored_state(state).unwrap())).unwrap()
    }

    fn registered(webauthn: &Webauthn, authenticator: &SoftPasskey) -> Passkey {
        let (options, registration) = start_registration(webauthn, &user(), Vec::new()).unwrap();
        let response = authenticator.create(&options, ORIGIN);

        webauthn
            .finish_passkey_registration(&response, &through_storage(&registration))
            .unwrap()
    }

    #[test]
    fn the_relying_party_is_the_external_address() {
        assert_eq!(relying_party("").err(), Some(Unavailable::NotConfigured));
        assert_eq!(
            relying_party("https://10.0.0.8").err(),
            Some(Unavailable::IpAddress)
        );
        assert_eq!(
            relying_party("https://[::1]:5080").err(),
            Some(Unavailable::IpAddress)
        );
        assert_eq!(
            relying_party("http://ops.example.com").err(),
            Some(Unavailable::NotHttps)
        );
        assert!(relying_party("http://localhost:5080").is_ok());

        let webauthn = relying_party("https://Ops.Example.com/console/").unwrap();
        let (options, _) = start_registration(&webauthn, &user(), Vec::new()).unwrap();
        assert_eq!(options.public_key.rp.id, "ops.example.com");
    }

    #[test]
    fn registration_asks_for_a_discoverable_passkey_without_cred_protect() {
        let webauthn = relying_party(ORIGIN).unwrap();
        let (options, _) = start_registration(&webauthn, &user(), Vec::new()).unwrap();
        let options = serde_json::to_value(&options).unwrap()["publicKey"].clone();

        assert_eq!(options["authenticatorSelection"]["residentKey"], "required");
        assert_eq!(
            options["authenticatorSelection"]["requireResidentKey"],
            true
        );
        assert_eq!(
            options["authenticatorSelection"]["userVerification"],
            "required"
        );
        assert!(options["authenticatorSelection"]["authenticatorAttachment"].is_null());
        assert_eq!(options["attestation"], "none");
        assert!(options["extensions"]["credentialProtectionPolicy"].is_null());
        assert_eq!(options["user"]["name"], "alice@example.com");
        assert_eq!(options["user"]["displayName"], "Alice");
    }

    #[test]
    fn a_synced_passkey_signs_in_without_a_username() {
        let webauthn = relying_party(ORIGIN).unwrap();
        let authenticator = SoftPasskey::new(user_handle(&user().id));
        let mut passkey = registered(&webauthn, &authenticator);

        let (options, state) = webauthn.start_discoverable_authentication().unwrap();
        let state: DiscoverableAuthentication = through_storage(&state);
        let response = authenticator.get(&options, ORIGIN);

        let (handle, credential_id) = webauthn
            .identify_discoverable_authentication(&response)
            .unwrap();
        assert_eq!(handle, user_handle(&user().id));
        assert_eq!(credential_id, passkey.cred_id().as_slice());

        let result = webauthn
            .finish_discoverable_authentication(
                &response,
                state,
                &[DiscoverableKey::from(&passkey)],
            )
            .unwrap();
        assert!(result.user_verified());
        assert!(passkey.update_credential(&result).is_some());
    }

    #[test]
    fn a_passkey_works_as_the_second_step() {
        let webauthn = relying_party(ORIGIN).unwrap();
        let authenticator = SoftPasskey::new(user_handle(&user().id));
        let passkey = registered(&webauthn, &authenticator);

        let (options, state) = webauthn
            .start_passkey_authentication(std::slice::from_ref(&passkey))
            .unwrap();
        let state: PasskeyAuthentication = through_storage(&state);

        assert!(
            webauthn
                .finish_passkey_authentication(&authenticator.get(&options, ORIGIN), &state)
                .is_ok()
        );
    }

    #[test]
    fn a_response_made_for_another_site_is_refused() {
        let webauthn = relying_party(ORIGIN).unwrap();
        let authenticator = SoftPasskey::new(user_handle(&user().id));
        let passkey = registered(&webauthn, &authenticator);

        let (options, state) = webauthn
            .start_passkey_authentication(std::slice::from_ref(&passkey))
            .unwrap();
        let phished = authenticator.get(&options, "https://ops.example.net");

        assert!(
            webauthn
                .finish_passkey_authentication(&phished, &state)
                .is_err()
        );
    }

    #[test]
    fn the_authenticator_model_comes_from_the_registration() {
        let webauthn = relying_party(ORIGIN).unwrap();
        let (options, _) = start_registration(&webauthn, &user(), Vec::new()).unwrap();
        let mut authenticator = SoftPasskey::new(user_handle(&user().id));

        assert_eq!(
            aaguid(&authenticator.create(&options, ORIGIN)).as_deref(),
            Some(ICLOUD_KEYCHAIN)
        );

        authenticator.aaguid = vec![0; 16];
        assert_eq!(aaguid(&authenticator.create(&options, ORIGIN)), None);
    }

    #[test]
    fn user_handles_are_stable_and_differ_between_accounts() {
        let alice = Id::from_string("2Yb8w0D3k1u9sQZpWm4cXrT7aLf");

        assert_eq!(user_handle(&alice), user_handle(&alice));
        assert_ne!(
            user_handle(&alice),
            user_handle(&Id::from_string("2Yb8w0D3k1u9sQZpWm4cXrT7aLg"))
        );
    }
}
