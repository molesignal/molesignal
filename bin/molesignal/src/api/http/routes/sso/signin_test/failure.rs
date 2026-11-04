// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试登录为什么没通过：分成前端能给说明和处理建议的类别，原话去掉敏感值后一起给管理员看。
//!
//! 登录流程的错误只有文字，所以按文字开头分类。分不出来的算 `unknown`，管理员还能看到原话，
//! 服务端内部的错误则不给原话。这里的测试用真的流程产生错误，文字改了会红。

use std::sync::LazyLock;

use regex::Regex;

use crate::{
    domain::iam::{SsoProvider, SsoProviderConfig},
    infra::sso::{TestedFailure, ldap::LdapFailure},
    shared::Error,
};

/// 发起测试之后服务配置改了。
pub(super) const SETTINGS_CHANGED: &str =
    "the provider's settings changed while this test was under way";

/// 组织只允许单点登录时的目录账号。
pub(super) const DIRECTORY_BLOCKED: &str =
    "this organization only allows single sign-on, so directory sign-ins cannot enter";

/// 引用了组织里已经没有的角色。
pub(super) const ROLE_MISSING: &str =
    "the role binding names a role this organization no longer has";

/// 原话最多留这么多字。
const MAX_DETAIL_CHARS: usize = 600;

/// 前端按它给说明和处理建议（`sso_providers.test.failures.<code>`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureCode {
    IdpError,
    NoCode,
    Network,
    TokenExchange,
    NoIdToken,
    IdTokenIssuer,
    IdTokenAudience,
    IdTokenExpired,
    IdTokenNonce,
    IdTokenInvalid,
    Jwks,
    Userinfo,
    ClaimMissing,
    SamlStatus,
    SamlNoResponse,
    SamlMalformed,
    SamlCertificate,
    SamlSignature,
    SamlTime,
    SamlAudience,
    SamlRecipient,
    LdapConnect,
    LdapServiceBind,
    LdapSearch,
    LdapNoEntry,
    LdapManyEntries,
    LdapMissingAttribute,
    LdapRejected,
    GroupNotAllowed,
    EmailUnverified,
    ProviderUnrestricted,
    InvalidEmail,
    EmailDomain,
    OrgDisabled,
    AccountUnavailable,
    AccountConfined,
    RoleMissing,
    DirectoryBlocked,
    SettingsChanged,
    Unknown,
}

impl FailureCode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::IdpError => "idp_error",
            Self::NoCode => "no_code",
            Self::Network => "network",
            Self::TokenExchange => "token_exchange",
            Self::NoIdToken => "no_id_token",
            Self::IdTokenIssuer => "id_token_issuer",
            Self::IdTokenAudience => "id_token_audience",
            Self::IdTokenExpired => "id_token_expired",
            Self::IdTokenNonce => "id_token_nonce",
            Self::IdTokenInvalid => "id_token_invalid",
            Self::Jwks => "jwks",
            Self::Userinfo => "userinfo",
            Self::ClaimMissing => "claim_missing",
            Self::SamlStatus => "saml_status",
            Self::SamlNoResponse => "saml_no_response",
            Self::SamlMalformed => "saml_malformed",
            Self::SamlCertificate => "saml_certificate",
            Self::SamlSignature => "saml_signature",
            Self::SamlTime => "saml_time",
            Self::SamlAudience => "saml_audience",
            Self::SamlRecipient => "saml_recipient",
            Self::LdapConnect => "ldap_connect",
            Self::LdapServiceBind => "ldap_service_bind",
            Self::LdapSearch => "ldap_search",
            Self::LdapNoEntry => "ldap_no_entry",
            Self::LdapManyEntries => "ldap_many_entries",
            Self::LdapMissingAttribute => "ldap_missing_attribute",
            Self::LdapRejected => "ldap_rejected",
            Self::GroupNotAllowed => "group_not_allowed",
            Self::EmailUnverified => "email_unverified",
            Self::ProviderUnrestricted => "provider_unrestricted",
            Self::InvalidEmail => "invalid_email",
            Self::EmailDomain => "email_domain",
            Self::OrgDisabled => "org_disabled",
            Self::AccountUnavailable => "account_unavailable",
            Self::AccountConfined => "account_confined",
            Self::RoleMissing => "role_missing",
            Self::DirectoryBlocked => "directory_blocked",
            Self::SettingsChanged => "settings_changed",
            Self::Unknown => "unknown",
        }
    }
}

/// 错误文字的开头和它的类别，先写的优先。
const RULES: &[(&str, FailureCode)] = &[
    (SETTINGS_CHANGED, FailureCode::SettingsChanged),
    (DIRECTORY_BLOCKED, FailureCode::DirectoryBlocked),
    (ROLE_MISSING, FailureCode::RoleMissing),
    (
        "the identity provider returned no authorization code",
        FailureCode::NoCode,
    ),
    ("the identity provider returned ", FailureCode::IdpError),
    ("the identity provider answered ", FailureCode::SamlStatus),
    (
        "the identity provider has not verified",
        FailureCode::EmailUnverified,
    ),
    ("oauth token http", FailureCode::Network),
    ("idp http ", FailureCode::Network),
    ("jwks ", FailureCode::Jwks),
    ("idp token endpoint", FailureCode::TokenExchange),
    ("idp returned no id_token", FailureCode::NoIdToken),
    ("idp ", FailureCode::Userinfo),
    ("id_token or userinfo", FailureCode::Userinfo),
    ("userinfo sub", FailureCode::Userinfo),
    ("id_token verify: InvalidIssuer", FailureCode::IdTokenIssuer),
    ("id_token issuer", FailureCode::IdTokenIssuer),
    (
        "id_token verify: InvalidAudience",
        FailureCode::IdTokenAudience,
    ),
    ("id_token audience", FailureCode::IdTokenAudience),
    (
        "id_token verify: ExpiredSignature",
        FailureCode::IdTokenExpired,
    ),
    (
        "id_token verify: ImmatureSignature",
        FailureCode::IdTokenExpired,
    ),
    ("id_token has", FailureCode::IdTokenExpired),
    ("id_token nonce", FailureCode::IdTokenNonce),
    ("id_token", FailureCode::IdTokenInvalid),
    ("identity is missing the mapped", FailureCode::ClaimMissing),
    ("SAML identity is missing", FailureCode::ClaimMissing),
    (
        "the GitHub account has no verified email",
        FailureCode::ClaimMissing,
    ),
    ("GitHub user has no id", FailureCode::ClaimMissing),
    ("the GitHub user endpoint", FailureCode::Userinfo),
    ("feishu user_info", FailureCode::Userinfo),
    ("callback missing SAMLResponse", FailureCode::SamlNoResponse),
    ("saml: idp_x509_cert", FailureCode::SamlCertificate),
    ("xmldsig pem", FailureCode::SamlCertificate),
    ("xmldsig x509", FailureCode::SamlCertificate),
    ("xmldsig spki", FailureCode::SamlCertificate),
    ("xmldsig: public key", FailureCode::SamlCertificate),
    ("xmldsig: unsupported PEM", FailureCode::SamlCertificate),
    (
        "xmldsig: embedded X509Certificate",
        FailureCode::SamlCertificate,
    ),
    (
        "xmldsig(c14n): embedded X509Certificate",
        FailureCode::SamlCertificate,
    ),
    ("saml: assertion", FailureCode::SamlTime),
    ("saml: subject confirmation expired", FailureCode::SamlTime),
    ("saml: audience", FailureCode::SamlAudience),
    (
        "saml: subject confirmation recipient",
        FailureCode::SamlRecipient,
    ),
    ("xmldsig: no <Assertion>", FailureCode::SamlMalformed),
    ("xmldsig utf8", FailureCode::SamlMalformed),
    ("xmldsig xml", FailureCode::SamlMalformed),
    ("xml parse", FailureCode::SamlMalformed),
    ("xmldsig", FailureCode::SamlSignature),
    ("saml", FailureCode::SamlMalformed),
    (
        "this account is not in a group",
        FailureCode::GroupNotAllowed,
    ),
    (
        "this provider accepts public accounts",
        FailureCode::ProviderUnrestricted,
    ),
    (
        "external identity does not contain",
        FailureCode::InvalidEmail,
    ),
    ("email domain is not permitted", FailureCode::EmailDomain),
    ("organization is disabled", FailureCode::OrgDisabled),
    ("user disabled", FailureCode::AccountUnavailable),
    ("account pending approval", FailureCode::AccountUnavailable),
    (
        "account registration was rejected",
        FailureCode::AccountUnavailable,
    ),
    (
        "authenticated user no longer exists",
        FailureCode::AccountUnavailable,
    ),
    (
        "this account was created by another",
        FailureCode::AccountConfined,
    ),
];

fn classify(message: &str) -> FailureCode {
    if message.ends_with("in the identity provider's JWKS") {
        return FailureCode::Jwks;
    }

    RULES
        .iter()
        .find(|(start, _)| message.starts_with(start))
        .map_or(FailureCode::Unknown, |(_, code)| *code)
}

/// 错误的文字，不带前面的类型。
fn message_of(error: &Error) -> String {
    match error {
        Error::NotFound(message)
        | Error::Conflict(message)
        | Error::InvalidArgument(message)
        | Error::Unauthorized(message)
        | Error::Forbidden(message)
        | Error::Internal(message)
        | Error::ResourceExhausted(message)
        | Error::Unavailable(message) => message.clone(),
        other => other.to_string(),
    }
}

pub(super) fn of_error(error: &Error, redactor: &Redactor) -> TestedFailure {
    let message = message_of(error);
    let code = classify(&message);

    // 服务端自己的毛病（数据库之类）和身份源无关，原话不给页面。
    let internal = matches!(error, Error::Internal(_) | Error::Other(_));
    let detail = (code != FailureCode::Unknown || !internal).then(|| redactor.scrub(&message));

    TestedFailure {
        code: code.as_str().to_owned(),
        detail,
    }
}

pub(super) fn of_directory(failure: &LdapFailure, redactor: &Redactor) -> TestedFailure {
    let (code, detail) = match failure {
        LdapFailure::Connect(text) => (FailureCode::LdapConnect, Some(text.clone())),
        LdapFailure::ServiceBind(text) => (FailureCode::LdapServiceBind, Some(text.clone())),
        LdapFailure::Search(text) => (FailureCode::LdapSearch, Some(text.clone())),
        LdapFailure::NoEntry | LdapFailure::BadInput => (FailureCode::LdapNoEntry, None),
        LdapFailure::ManyEntries => (FailureCode::LdapManyEntries, None),
        LdapFailure::MissingAttribute { field, attribute } => (
            FailureCode::LdapMissingAttribute,
            Some(format!("{field} is mapped to the attribute `{attribute}`")),
        ),
        LdapFailure::Rejected(text) => (FailureCode::LdapRejected, Some(text.clone())),
    };

    TestedFailure {
        code: code.as_str().to_owned(),
        detail: detail.map(|detail| redactor.scrub(&detail)),
    }
}

/// 字段名后面跟着的值：令牌、密钥、断言。
static SECRET_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(access_token|refresh_token|id_token|client_secret|samlresponse|assertion|password)("?\s*[:=]\s*"?)[^\s"&,}]+"#,
    )
    .expect("a valid pattern")
});

/// 不像人话的长串：令牌和摘要多半长这样。
static OPAQUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+=-]{32,}").expect("a valid pattern"));

/// 身份源错误里常见的跟踪号，查问题用得上，不是秘密。
static GUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .expect("a valid pattern")
});

/// 把不能露出来的值从文字里去掉：这个服务的密钥、这次回调里见过的值，再加上看着像令牌的东西。
pub(super) struct Redactor {
    known: Vec<String>,
}

impl Redactor {
    pub(super) fn new(provider: &SsoProvider, seen: &[&str]) -> Self {
        let secret = match &provider.config {
            SsoProviderConfig::Oidc(config) => config.client_secret.as_str(),
            SsoProviderConfig::Oauth2(config) => config.client_secret.as_str(),
            SsoProviderConfig::Saml(_) => "",
            SsoProviderConfig::Ldap(config) => config.bind_password.as_str(),
        };

        let known = seen
            .iter()
            .chain(std::iter::once(&secret))
            .filter(|value| !value.is_empty())
            .map(|value| (*value).to_owned())
            .collect();

        Self { known }
    }

    pub(super) fn scrub(&self, text: &str) -> String {
        let mut scrubbed = text
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();

        for value in &self.known {
            scrubbed = scrubbed.replace(value, "***");
        }

        let scrubbed = SECRET_FIELD.replace_all(&scrubbed, "$1$2***");

        let scrubbed = OPAQUE.replace_all(&scrubbed, |found: &regex::Captures<'_>| {
            let run = &found[0];

            if GUID.is_match(run) {
                run.to_owned()
            } else {
                "***".to_owned()
            }
        });

        scrubbed.chars().take(MAX_DETAIL_CHARS).collect()
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::json;

    use super::*;
    use crate::{
        domain::iam::{
            SsoFieldMapping, SsoLdapConfig, SsoOidcConfig, SsoPlatform, SsoProviderKind,
        },
        infra::sso::{
            OAuth2LoginFlow, OidcLoginFlow, SamlConfig, SamlLoginFlow,
            claims::map_identity,
            oidc::id_token::{Expected, verify},
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn provider(config: SsoProviderConfig) -> SsoProvider {
        SsoProvider {
            id: Id::from_string("p-1"),
            org_id: Id::from_string("org-1"),
            name: "Okta".into(),
            kind: SsoProviderKind::Oidc,
            platform: SsoPlatform::Custom,
            enabled: true,
            config,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn oidc_provider(secret: &str) -> SsoProvider {
        provider(SsoProviderConfig::Oidc(SsoOidcConfig {
            client_secret: secret.into(),
            ..SsoOidcConfig::default()
        }))
    }

    fn redactor() -> Redactor {
        Redactor::new(&oidc_provider("hunter2-secret"), &["the-code"])
    }

    fn code_of(error: &Error) -> FailureCode {
        classify(&message_of(error))
    }

    fn unsigned_token(claims: serde_json::Value) -> String {
        let encode = |value: &serde_json::Value| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string())
        };

        format!(
            "{}.{}.sig",
            encode(&json!({ "alg": "RS256", "typ": "JWT" })),
            encode(&claims)
        )
    }

    async fn token_error(claims: serde_json::Value) -> Error {
        let expected = Expected {
            issuer: "https://idp.example",
            audience: "molesignal",
            nonce: "n-1",
            client_secret: "",
        };

        verify(&unsigned_token(claims), &expected, None)
            .await
            .expect_err("the token must be refused")
    }

    #[tokio::test]
    async fn id_token_refusals_name_what_to_look_at() {
        let far_future = 4_102_444_800_i64;

        let issuer = token_error(
            json!({ "iss": "https://other.example", "aud": "molesignal", "nonce": "n-1", "exp": far_future }),
        )
        .await;
        assert_eq!(code_of(&issuer), FailureCode::IdTokenIssuer);

        let audience = token_error(
            json!({ "iss": "https://idp.example", "aud": "someone-else", "nonce": "n-1", "exp": far_future }),
        )
        .await;
        assert_eq!(code_of(&audience), FailureCode::IdTokenAudience);

        let nonce = token_error(
            json!({ "iss": "https://idp.example", "aud": "molesignal", "nonce": "wrong", "exp": far_future }),
        )
        .await;
        assert_eq!(code_of(&nonce), FailureCode::IdTokenNonce);

        let expired = token_error(
            json!({ "iss": "https://idp.example", "aud": "molesignal", "nonce": "n-1", "exp": 1 }),
        )
        .await;
        assert_eq!(code_of(&expired), FailureCode::IdTokenExpired);

        let no_exp = token_error(
            json!({ "iss": "https://idp.example", "aud": "molesignal", "nonce": "n-1" }),
        )
        .await;
        assert_eq!(code_of(&no_exp), FailureCode::IdTokenExpired);
    }

    #[test]
    fn jsonwebtoken_answers_are_read_by_their_kind() {
        for (message, code) in [
            ("id_token verify: InvalidIssuer", FailureCode::IdTokenIssuer),
            (
                "id_token verify: InvalidAudience",
                FailureCode::IdTokenAudience,
            ),
            (
                "id_token verify: ExpiredSignature",
                FailureCode::IdTokenExpired,
            ),
            (
                "id_token verify: ImmatureSignature",
                FailureCode::IdTokenExpired,
            ),
            (
                "id_token verify: InvalidSignature",
                FailureCode::IdTokenInvalid,
            ),
            ("id_token header: InvalidToken", FailureCode::IdTokenInvalid),
            (
                "no RS256 key with kid `k1` in the identity provider's JWKS",
                FailureCode::Jwks,
            ),
            ("jwks fetch status 404 Not Found", FailureCode::Jwks),
        ] {
            assert_eq!(classify(message), code, "{message}");
        }
    }

    #[test]
    fn a_missing_mapped_claim_is_a_mapping_problem() {
        let error =
            map_identity(&json!({ "sub": "u-1" }), &SsoFieldMapping::oidc()).expect_err("no email");

        assert_eq!(code_of(&error), FailureCode::ClaimMissing);
        assert!(message_of(&error).contains("`email`"));
    }

    #[tokio::test]
    async fn a_token_endpoint_that_cannot_be_reached_is_a_network_problem() {
        // 地址不合法，请求发不出去：不靠网络，也不受环境里代理的影响。
        let flow = OidcLoginFlow::new(&SsoOidcConfig {
            authorize_url: "not a url".into(),
            token_url: "not a url".into(),
            client_id: "molesignal".into(),
            redirect_uri: "not a url".into(),
            ..SsoOidcConfig::default()
        })
        .expect("flow");

        let cache = crate::infra::sso::JwksCache::new(60).expect("cache");
        let error = flow
            .identity("code", &cache, "n-1")
            .await
            .expect_err("refused");

        assert_eq!(code_of(&error), FailureCode::Network);

        let flow = OAuth2LoginFlow::new(
            &crate::domain::iam::SsoOauth2Config {
                authorize_url: "not a url".into(),
                token_url: "not a url".into(),
                userinfo_url: "not a url".into(),
                client_id: "molesignal".into(),
                redirect_uri: "not a url".into(),
                ..crate::domain::iam::SsoOauth2Config::default()
            },
            SsoPlatform::Custom,
        )
        .expect("flow");

        let error = flow.identity("code").await.expect_err("refused");
        assert_eq!(code_of(&error), FailureCode::Network);
    }

    fn saml_flow() -> SamlLoginFlow {
        SamlLoginFlow::new(SamlConfig {
            sp_entity_id: "https://sp.local".into(),
            idp_entity_id: "https://idp.example".into(),
            idp_sso_url: "https://idp.example/sso".into(),
            idp_x509_cert: "-----BEGIN CERTIFICATE-----\nABCD\n-----END CERTIFICATE-----".into(),
            assertion_consumer_url: "https://sp.local/acs".into(),
            field_mapping: SsoFieldMapping::saml(),
        })
    }

    fn encoded(xml: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(xml)
    }

    #[test]
    fn saml_refusals_tell_a_broken_response_from_a_bad_signature() {
        let flow = saml_flow();

        let not_base64 = flow.parse_response("%%%").expect_err("refused");
        assert_eq!(code_of(&not_base64), FailureCode::SamlMalformed);

        let no_assertion = flow
            .parse_response(&encoded("<Response/>"))
            .expect_err("refused");
        assert_eq!(code_of(&no_assertion), FailureCode::SamlMalformed);

        let unsigned = flow
            .parse_response(&encoded(
                "<Response><Assertion><NameID>a@example.com</NameID></Assertion></Response>",
            ))
            .expect_err("refused");
        assert_eq!(code_of(&unsigned), FailureCode::SamlSignature);
    }

    #[test]
    fn saml_condition_answers_are_read_by_their_kind() {
        for (message, code) in [
            (
                "saml: assertion expired (Conditions/NotOnOrAfter)",
                FailureCode::SamlTime,
            ),
            (
                "saml: assertion not yet valid (Conditions/NotBefore)",
                FailureCode::SamlTime,
            ),
            ("saml: subject confirmation expired", FailureCode::SamlTime),
            (
                "saml: audience restriction does not include this SP",
                FailureCode::SamlAudience,
            ),
            (
                "saml: subject confirmation recipient mismatch",
                FailureCode::SamlRecipient,
            ),
            (
                "saml: idp_x509_cert not configured",
                FailureCode::SamlCertificate,
            ),
            ("xmldsig pem parse: bad", FailureCode::SamlCertificate),
            (
                "xmldsig: embedded X509Certificate does not match configured IdP cert",
                FailureCode::SamlCertificate,
            ),
            (
                "xmldsig: reference digest mismatch (Assertion may have been tampered)",
                FailureCode::SamlSignature,
            ),
            (
                "xmldsig RSA verify: Unspecified",
                FailureCode::SamlSignature,
            ),
            ("callback missing SAMLResponse", FailureCode::SamlNoResponse),
            (
                "the identity provider answered Responder: no",
                FailureCode::SamlStatus,
            ),
            (
                "SAML identity is missing mapped email field `mail`",
                FailureCode::ClaimMissing,
            ),
        ] {
            assert_eq!(classify(message), code, "{message}");
        }
    }

    #[test]
    fn redirect_answers_are_read_by_their_kind() {
        for (message, code) in [
            (
                "the identity provider returned access_denied: nope",
                FailureCode::IdpError,
            ),
            (
                "the identity provider returned no authorization code",
                FailureCode::NoCode,
            ),
            (
                "idp token endpoint 401 Unauthorized: invalid_client",
                FailureCode::TokenExchange,
            ),
            ("idp returned no id_token", FailureCode::NoIdToken),
            (
                "idp https://idp.example/me 403 Forbidden: scope",
                FailureCode::Userinfo,
            ),
            (
                "idp https://idp.example/me did not return JSON",
                FailureCode::Userinfo,
            ),
            (
                "idp http https://idp.example/me: timed out",
                FailureCode::Network,
            ),
            (
                "oauth token http: error sending request",
                FailureCode::Network,
            ),
            (
                "userinfo sub does not match the id_token sub",
                FailureCode::Userinfo,
            ),
            (
                "feishu user_info returned code 20003: invalid code",
                FailureCode::Userinfo,
            ),
            (
                "the GitHub account has no verified email address, or the user:email scope was not granted",
                FailureCode::ClaimMissing,
            ),
        ] {
            assert_eq!(classify(message), code, "{message}");
        }
    }

    #[test]
    fn text_from_an_identity_provider_cannot_steer_the_class() {
        assert_eq!(
            classify("idp token endpoint 400 Bad Request: id_token jwks saml xmldsig"),
            FailureCode::TokenExchange
        );
    }

    #[test]
    fn rule_answers_are_read_by_their_kind() {
        for (message, code) in [
            (
                "this account is not in a group allowed to sign in with this provider",
                FailureCode::GroupNotAllowed,
            ),
            (
                "the identity provider has not verified this account's email address",
                FailureCode::EmailUnverified,
            ),
            (
                "this provider accepts public accounts: add the organization's email domains",
                FailureCode::ProviderUnrestricted,
            ),
            (
                "external identity does not contain a valid email address",
                FailureCode::InvalidEmail,
            ),
            (
                "email domain is not permitted to join this organization",
                FailureCode::EmailDomain,
            ),
            (
                "organization is disabled by a platform administrator",
                FailureCode::OrgDisabled,
            ),
            ("user disabled", FailureCode::AccountUnavailable),
            ("account pending approval", FailureCode::AccountUnavailable),
            (
                "this account was created by another organization's single sign-on",
                FailureCode::AccountConfined,
            ),
            (SETTINGS_CHANGED, FailureCode::SettingsChanged),
            (DIRECTORY_BLOCKED, FailureCode::DirectoryBlocked),
            (ROLE_MISSING, FailureCode::RoleMissing),
        ] {
            assert_eq!(classify(message), code, "{message}");
        }
    }

    #[test]
    fn an_unknown_internal_error_gives_no_text_to_the_page() {
        let failure = of_error(&Error::internal("db password=abc refused"), &redactor());

        assert_eq!(failure.code, "unknown");
        assert_eq!(failure.detail, None);

        let failure = of_error(&Error::invalid("something odd"), &redactor());

        assert_eq!(failure.code, "unknown");
        assert_eq!(failure.detail.as_deref(), Some("something odd"));
    }

    #[test]
    fn a_known_failure_keeps_the_identity_providers_words() {
        let failure = of_error(
            &Error::unauthorized("idp token endpoint 401 Unauthorized: invalid_client"),
            &redactor(),
        );

        assert_eq!(failure.code, "token_exchange");
        assert_eq!(
            failure.detail.as_deref(),
            Some("idp token endpoint 401 Unauthorized: invalid_client")
        );
    }

    #[test]
    fn the_secrets_of_this_test_never_reach_the_text() {
        let scrubbed = redactor().scrub("bad secret hunter2-secret for code the-code");

        assert!(!scrubbed.contains("hunter2-secret"));
        assert!(!scrubbed.contains("the-code"));
    }

    #[test]
    fn the_directory_password_never_reaches_the_text() {
        let provider = provider(SsoProviderConfig::Ldap(SsoLdapConfig {
            bind_password: "reader-pass".into(),
            ..SsoLdapConfig::default()
        }));

        let redactor = Redactor::new(&provider, &["tested-pass"]);
        let scrubbed = redactor.scrub("bind with reader-pass and tested-pass failed");

        assert_eq!(scrubbed, "bind with *** and *** failed");
    }

    #[test]
    fn values_behind_secret_field_names_are_hidden() {
        for text in [
            r#"{"access_token":"abc","error":"bad"}"#,
            "access_token=abc&error=bad",
            r#"{"ID_TOKEN": "abc"}"#,
            "client_secret=abc",
            "password: abc",
        ] {
            let scrubbed = redactor().scrub(text);
            assert!(!scrubbed.contains("abc"), "{text} became {scrubbed}");
            assert!(scrubbed.contains("***"), "{text} became {scrubbed}");
        }
    }

    #[test]
    fn long_opaque_runs_go_and_ordinary_words_and_trace_ids_stay() {
        let jwt = format!("{}.{}", "e".repeat(40), "p".repeat(80));
        let scrubbed = redactor().scrub(&format!("token {jwt} refused"));
        assert_eq!(scrubbed, "token ***.*** refused");

        let readable = "AADSTS50011: The redirect URI https://obs.example.com/api/v1/auth/sso/callback does not match. Trace ID: 2f1b6a14-3c7d-4e21-9a0b-6c1d2e3f4a5b";
        assert_eq!(redactor().scrub(readable), readable);
    }

    #[test]
    fn the_text_is_cut_and_has_no_control_characters() {
        let scrubbed = redactor().scrub(&format!("line\nbreak {}", "word ".repeat(400)));

        assert!(scrubbed.starts_with("linebreak word"));
        assert_eq!(scrubbed.chars().count(), MAX_DETAIL_CHARS);
    }

    #[test]
    fn directory_failures_name_their_step() {
        let redactor = redactor();

        for (failure, code) in [
            (LdapFailure::Connect("refused".into()), "ldap_connect"),
            (
                LdapFailure::ServiceBind("rc=49".into()),
                "ldap_service_bind",
            ),
            (LdapFailure::Search("rc=32".into()), "ldap_search"),
            (LdapFailure::NoEntry, "ldap_no_entry"),
            (LdapFailure::ManyEntries, "ldap_many_entries"),
            (LdapFailure::Rejected("rc=49".into()), "ldap_rejected"),
        ] {
            assert_eq!(of_directory(&failure, &redactor).code, code);
        }

        let missing = of_directory(
            &LdapFailure::MissingAttribute {
                field: "email",
                attribute: "mail".into(),
            },
            &redactor,
        );

        assert_eq!(missing.code, "ldap_missing_attribute");
        assert!(missing.detail.expect("detail").contains("`mail`"));
    }
}
