// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 预算对有人在等的请求的两种表达：用完时的 429 应答，接近或超过时随回答发出的提醒。
//! 两者都只给数字和一个固定的错误码，说明文字由前端按读者语言写。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::agent::usage::BudgetStatus;

/// 前端据此认出"今天的预算用完了"。
pub(crate) const BUDGET_EXHAUSTED: &str = "agent_budget_exhausted";

pub(crate) fn exhausted_response(status: &BudgetStatus) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "error": BUDGET_EXHAUSTED,
            "code": BUDGET_EXHAUSTED,
            "message": "the organization's daily Mole Agent token budget is used up",
            "used_tokens": status.used_tokens,
            "limit_tokens": status.limit_tokens,
            "resets_at_micros": status.resets_at.0,
        })),
    )
        .into_response()
}

/// 接近或超过上限时的提醒；还早时为空。
pub(crate) fn notice(status: &BudgetStatus) -> Option<Value> {
    status.should_warn().then(|| {
        json!({
            "kind": "budget",
            "level": status.level,
            "used_tokens": status.used_tokens,
            "limit_tokens": status.limit_tokens,
            "resets_at_micros": status.resets_at.0,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::usage::{BudgetStatus, InteractiveBudgetMode, UsageDay};

    #[test]
    fn a_notice_appears_from_the_warning_threshold_on() {
        let status =
            |used| BudgetStatus::evaluate(used, 100, 80, InteractiveBudgetMode::Warn, UsageDay(10));

        assert!(notice(&status(50)).is_none());

        let warning = notice(&status(85)).unwrap();
        assert_eq!(warning["level"], "warning");
        assert_eq!(warning["used_tokens"], 85);
        assert_eq!(warning["resets_at_micros"], UsageDay(11).start().0);

        assert_eq!(notice(&status(120)).unwrap()["level"], "exhausted");
    }

    #[test]
    fn the_refusal_is_a_429_with_a_stable_code() {
        let status =
            BudgetStatus::evaluate(100, 100, 80, InteractiveBudgetMode::Block, UsageDay(1));

        assert_eq!(
            exhausted_response(&status).status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
