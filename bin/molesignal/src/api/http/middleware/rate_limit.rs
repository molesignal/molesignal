// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! router 角色的按组织限流：`[router.rate_limit]` 分别限制写入（`/api/v1/intake/*`）与
//! 查询（`/api/v1/query*`）的请求速率。
//!
//! 令牌桶按 `(组织, 路由类别)` 独立计数，组织取自已通过鉴权的 [`IamContext`]；超限返回
//! `429` 与 `Retry-After`。计数在每个 router 副本内独立。

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use axum::{
    extract::{Request, State},
    http::HeaderValue,
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    state::{InMemoryState, NotKeyed},
};
use parking_lot::Mutex;

use crate::{app::iam::IamContext, config::RouterRateLimit, shared::Error};

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum RouteClass {
    Intake,
    Query,
}

impl RouteClass {
    fn of(path: &str) -> Option<Self> {
        if path.starts_with("/api/v1/intake/") {
            Some(Self::Intake)
        } else if path == "/api/v1/query" || path.starts_with("/api/v1/query/") {
            Some(Self::Query)
        } else {
            None
        }
    }
}

pub struct RouterRateLimits {
    rate: RouterRateLimit,
    limiters: Mutex<HashMap<(String, RouteClass), Arc<Limiter>>>,
}

impl RouterRateLimits {
    pub fn new(rate: RouterRateLimit) -> Arc<Self> {
        Arc::new(Self {
            rate,
            limiters: Mutex::new(HashMap::new()),
        })
    }

    fn limiter(&self, org_id: &str, class: RouteClass) -> Option<Arc<Limiter>> {
        let qps = match class {
            RouteClass::Intake => self.rate.intake_qps,
            RouteClass::Query => self.rate.query_qps,
        };
        let qps = NonZeroU32::new(qps)?;
        let burst = qps
            .get()
            .saturating_mul(self.rate.burst_multiplier)
            .max(qps.get());
        let mut limiters = self.limiters.lock();
        let limiter = limiters
            .entry((org_id.to_owned(), class))
            .or_insert_with(|| {
                let quota = Quota::per_second(qps)
                    .allow_burst(NonZeroU32::new(burst).expect("burst is at least qps"));
                Arc::new(RateLimiter::direct(quota))
            });
        Some(limiter.clone())
    }

    /// `None` 表示放行，`Some(秒)` 是建议的重试等待。
    fn check(&self, org_id: &str, class: RouteClass) -> Option<u64> {
        let limiter = self.limiter(org_id, class)?;
        limiter.check().err().map(|not_until| {
            not_until
                .wait_time_from(DefaultClock::default().now())
                .as_secs()
                .max(1)
        })
    }
}

/// 放在鉴权层之内：此时请求已带有 [`IamContext`]。
pub async fn rate_limit_layer(
    State(limits): State<Arc<RouterRateLimits>>,
    req: Request,
    next: Next,
) -> Response {
    let class = RouteClass::of(req.uri().path());
    let org_id = req
        .extensions()
        .get::<IamContext>()
        .map(|context| context.org_id.0.clone());
    if let (Some(class), Some(org_id)) = (class, org_id)
        && let Some(retry_after) = limits.check(&org_id, class)
    {
        let mut response = Error::resource_exhausted("rate limit exceeded").into_response();
        response.headers_mut().insert(
            "Retry-After",
            HeaderValue::from_str(&retry_after.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
        return response;
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(intake_qps: u32, query_qps: u32, burst_multiplier: u32) -> Arc<RouterRateLimits> {
        RouterRateLimits::new(RouterRateLimit {
            intake_qps,
            query_qps,
            burst_multiplier,
        })
    }

    #[test]
    fn classifies_intake_and_query_paths() {
        assert_eq!(
            RouteClass::of("/api/v1/intake/logs/app"),
            Some(RouteClass::Intake)
        );
        assert_eq!(RouteClass::of("/api/v1/query"), Some(RouteClass::Query));
        assert_eq!(
            RouteClass::of("/api/v1/query/stream"),
            Some(RouteClass::Query)
        );
        assert_eq!(RouteClass::of("/api/v1/queryx"), None);
        assert_eq!(RouteClass::of("/api/v1/streams"), None);
    }

    #[test]
    fn blocks_once_the_burst_is_used_up() {
        let limits = limits(2, 2, 1);
        assert!(limits.check("org", RouteClass::Intake).is_none());
        assert!(limits.check("org", RouteClass::Intake).is_none());
        let retry_after = limits.check("org", RouteClass::Intake).expect("limited");
        assert!(retry_after >= 1);
    }

    #[test]
    fn organizations_and_classes_are_counted_separately() {
        let limits = limits(1, 1, 1);
        assert!(limits.check("a", RouteClass::Intake).is_none());
        assert!(limits.check("a", RouteClass::Intake).is_some());
        assert!(limits.check("b", RouteClass::Intake).is_none());
        assert!(limits.check("a", RouteClass::Query).is_none());
    }

    #[test]
    fn zero_disables_the_limit_of_that_class() {
        let limits = limits(0, 1, 1);
        for _ in 0..100 {
            assert!(limits.check("org", RouteClass::Intake).is_none());
        }
        assert!(limits.check("org", RouteClass::Query).is_none());
        assert!(limits.check("org", RouteClass::Query).is_some());
    }
}
