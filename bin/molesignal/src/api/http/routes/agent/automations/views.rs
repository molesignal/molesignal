// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 页面读到的规则：规则本身，加上读的人能做什么。

use serde::Serialize;

use super::access::{can_edit, can_moderate};
use crate::{
    agent::{automations::Automation, scope::Viewer},
    app::iam::IamContext,
};

#[derive(Debug, Serialize)]
pub(super) struct AutomationView {
    #[serde(flatten)]
    pub(super) automation: Automation,
    /// 读的人能改它、手动运行它、管理它的令牌。
    pub(super) editable: bool,
    /// 读的人能停用、删除它。
    pub(super) moderatable: bool,
}

impl AutomationView {
    pub(super) fn of(automation: Automation, ctx: &IamContext, viewer: &Viewer) -> Self {
        Self {
            editable: can_edit(ctx, viewer, &automation.scope),
            moderatable: can_moderate(ctx, viewer, &automation.scope),
            automation,
        }
    }
}
