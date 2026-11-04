// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 飞书 / Lark 的身份读取。
//!
//! 令牌接口（`authen/v2/oauth/token`）只收 JSON 请求体，由 [`super::OAuth2LoginFlow`]
//! 按平台选编码；用户信息（`authen/v1/user_info`）包在 `{code, msg, data}` 里，`code`
//! 非 0 表示失败，字段映射按 `data.*` 路径取值。

use serde_json::Value;

use crate::{
    domain::iam::SsoFieldMapping,
    infra::sso::claims::{ExternalIdentity, map_identity},
    shared::{Error, Result},
};

pub(super) fn identity(user: &Value, mapping: &SsoFieldMapping) -> Result<ExternalIdentity> {
    let code = user.get("code").and_then(Value::as_i64).unwrap_or(0);

    if code != 0 {
        let msg = user.get("msg").and_then(Value::as_str).unwrap_or_default();

        return Err(Error::unauthorized(format!(
            "feishu user_info returned code {code}: {msg}"
        )));
    }

    map_identity(user, mapping)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mapping() -> SsoFieldMapping {
        SsoFieldMapping {
            subject: "data.union_id".into(),
            email: "data.enterprise_email | data.email".into(),
            display_name: "data.name".into(),
            groups: "data.groups".into(),
        }
    }

    #[test]
    fn reads_the_wrapped_user() {
        let user = json!({
            "code": 0,
            "msg": "success",
            "data": {
                "union_id": "on_1",
                "open_id": "ou_1",
                "name": "张三",
                "email": "zhangsan@example.com"
            }
        });
        let identity = identity(&user, &mapping()).expect("identity");
        assert_eq!(identity.subject, "on_1");
        assert_eq!(identity.email, "zhangsan@example.com");
        assert_eq!(identity.name.as_deref(), Some("张三"));
        assert!(identity.groups.is_empty());
    }

    #[test]
    fn a_non_zero_code_is_an_error() {
        let user = json!({ "code": 99991663, "msg": "invalid access token" });
        let error = identity(&user, &mapping()).expect_err("error code");
        assert!(error.to_string().contains("99991663"));
    }
}
