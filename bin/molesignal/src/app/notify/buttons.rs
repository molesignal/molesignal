// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警通知上的按钮从哪里来：只有开了「在 IM 里回答」的飞书 App、Slack App 收得到点击，按钮上的
//! 字按收的人的语言写。Mole Agent 在通知引擎之后才建好，引擎拿着一个槽，bootstrap 建好后填上；
//! 没填时不画按钮。

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::{
    domain::notify::{buttons::AlertButtonLabels, connector::NotifyConnector},
    shared::ids::Id,
};

/// 收得到回复的短信与电话末尾说的那句：短信回复哪个代码，电话按哪个键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyHints {
    pub sms: String,
    pub voice: String,
}

#[async_trait]
pub trait AlertButtonSource: Send + Sync {
    /// `connector` 发出的告警通知上按钮的字；它收不到点击时为 `None`。`recipient` 是私聊收通知的
    /// 人，发到群里时为 `None`，用工作区的语言。
    async fn labels(
        &self,
        connector: &NotifyConnector,
        recipient: Option<&Id>,
    ) -> Option<AlertButtonLabels>;

    /// 回复提示，用 `recipient` 的语言（没有时用工作区的）；`ack`、`resolve` 是短信的两个代码。
    async fn reply_hints(
        &self,
        organization_id: &Id,
        recipient: Option<&Id>,
        ack: &str,
        resolve: &str,
    ) -> Option<ReplyHints> {
        let _ = (organization_id, recipient, ack, resolve);
        None
    }
}

/// 引擎找按钮的地方。
#[derive(Default)]
pub struct AlertButtonSlot(OnceLock<Arc<dyn AlertButtonSource>>);

impl AlertButtonSlot {
    pub fn fill(&self, source: Arc<dyn AlertButtonSource>) {
        let _ = self.0.set(source);
    }

    pub(super) fn source(&self) -> Option<&Arc<dyn AlertButtonSource>> {
        self.0.get()
    }
}
