// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉建群：重大事件的作战室用应用的凭证建一个内部群。

use serde_json::{Value, json};

use super::DingTalkApp;
use crate::{
    infra::notify::adapters::common::{provider_message, truncate_chars},
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

impl DingTalkApp {
    /// 建一个内部群：`owner` 当群主，`members`（userId）拉进来，群名最多 20 个字；返回群的
    /// openConversationId。应用的机器人不会随之进群，要在钉钉里把它加进去，消息才发得进来。
    pub(crate) async fn create_group(
        &self,
        http: &reqwest::Client,
        name: &str,
        owner: &str,
        members: &[String],
    ) -> Result<String> {
        let token = self.access_token(http).await?;

        let response = http_trace::send(
            http,
            http.post(format!("{}/v1.0/im/group/create", self.base()))
                .header("x-acs-dingtalk-access-token", token)
                .json(&json!({
                    "name": truncate_chars(name, 20),
                    "owner": owner,
                    "useridlist": members,
                })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("dingtalk_app group request failed"))?;

        let status = response.status();
        let created: Value = response.json().await.unwrap_or(Value::Null);

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "dingtalk_app could not create the group: HTTP {} {}",
                status.as_u16(),
                provider_message(created["message"].as_str().unwrap_or_default())
            )));
        }

        created["openConversationId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::unavailable("dingtalk_app did not return the group"))
    }
}
