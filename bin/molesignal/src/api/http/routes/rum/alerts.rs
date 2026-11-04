// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /rum/alerts/preview`: what a rule on a RUM figure would have seen.
//!
//! The editor draws the figure over the last stretch of time with the thresholds on it, and the
//! templates say what the figure is now. Both read here, because the figure is worked out by the
//! code that evaluates the rule: the curve, the value now and what the rule would have done agree
//! with what the rule does once it runs.

use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};

use super::reader::within_budget;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        rum::{RumAlertMetric, RumAlertQuery},
    },
    infra::rum::query::Unavailable,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const HOUR_MICROS: i64 = 3_600_000_000;

/// How far back a preview looks unless the request says: about six hours, in whole windows, and
/// at least a dozen of them for a long window, but never more than a day.
const DEFAULT_LOOKBACK_MICROS: i64 = 6 * HOUR_MICROS;
const MAX_LOOKBACK_MICROS: i64 = 24 * HOUR_MICROS;
const LEAST_WINDOWS: i64 = 12;
const MOST_WINDOWS: i64 = 72;

/// The most windows a preview holds.
const MAX_WINDOWS: i64 = 360;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewRequest {
    query: RumAlertQuery,
    /// The window the rule is evaluated over, which is how long a point of the curve is.
    period_secs: u32,
    /// Microseconds; the last stretch of time ending now when absent.
    from: Option<i64>,
    to: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct Point {
    pub at: i64,
    /// None where the figure has no value in the window: nothing happened, or too few rows.
    pub value: Option<f64>,
    pub samples: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Preview {
    metric: RumAlertMetric,
    unit: &'static str,
    /// How many rows the figure needs in a window before it has a value.
    min_samples: u32,
    period_secs: u32,
    from: i64,
    to: i64,
    points: Vec<Point>,
    /// The last window, which ends at `to`.
    current: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

/// The windows to read: `[from, to)` cut into whole windows of `bar` microseconds, the last ending
/// at `to`.
fn windows(request: &PreviewRequest, bar: i64, now: i64) -> Result<(i64, i64)> {
    let to = request.to.unwrap_or(now);

    let from = match request.from {
        Some(from) => from,
        None => {
            let wanted = (DEFAULT_LOOKBACK_MICROS / bar).clamp(LEAST_WINDOWS, MOST_WINDOWS);
            let allowed = (MAX_LOOKBACK_MICROS / bar).max(1);

            to - wanted.min(allowed) * bar
        }
    };

    if to <= from {
        return Err(Error::invalid("`to` must be after `from`"));
    }

    if (to - from + bar - 1) / bar > MAX_WINDOWS {
        return Err(Error::invalid(format!(
            "a preview holds at most {MAX_WINDOWS} windows: shorten the range"
        )));
    }

    Ok((from, to))
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn preview(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<PreviewRequest>,
) -> Result<Json<Preview>> {
    request.query.validate(request.period_secs)?;

    let bar = i64::from(request.period_secs) * 1_000_000;
    let (from, to) = windows(&request, bar, TimestampMicros::now().0)?;

    let series = within_budget(state.alerting.rum.series(
        &iam.org_id,
        &request.query,
        TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        bar,
    ))
    .await?;

    let (points, unavailable) = match series {
        Ok(points) => (
            points
                .into_iter()
                .map(|point| Point {
                    at: point.at.0,
                    value: point.reading.as_ref().map(|reading| reading.value),
                    samples: point.reading.as_ref().map(|reading| reading.samples),
                })
                .collect::<Vec<_>>(),
            None,
        ),
        Err(reason) => (Vec::new(), Some(reason)),
    };

    let current = points.last().map(|point| Point {
        at: point.at,
        value: point.value,
        samples: point.samples,
    });

    Ok(Json(Preview {
        metric: request.query.metric,
        unit: request.query.unit(),
        min_samples: request.query.effective_min_samples(),
        period_secs: request.period_secs,
        from,
        to,
        points,
        current,
        unavailable,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::rum::RumAlertMetric;

    fn request(from: Option<i64>, to: Option<i64>) -> PreviewRequest {
        PreviewRequest {
            query: RumAlertQuery {
                application_id: "shop-web".into(),
                environment: None,
                page: None,
                version: None,
                metric: RumAlertMetric::ErrorRate,
                min_samples: None,
                custom_name: None,
                statistic: None,
                search: None,
            },
            period_secs: 900,
            from,
            to,
        }
    }

    const NOW: i64 = 1_760_000_000_000_000;
    const FIFTEEN_MINUTES: i64 = 900_000_000;

    #[test]
    fn the_default_is_whole_windows_ending_now() {
        let (from, to) = windows(&request(None, None), FIFTEEN_MINUTES, NOW).unwrap();

        assert_eq!(to, NOW);
        // Six hours of fifteen-minute windows.
        assert_eq!((to - from) / FIFTEEN_MINUTES, 24);
    }

    #[test]
    fn a_long_window_looks_back_over_a_dozen_of_them_but_not_past_a_day() {
        let hour = HOUR_MICROS;
        let (from, to) = windows(&request(None, None), hour, NOW).unwrap();

        assert_eq!((to - from) / hour, 12);

        let six_hours = 6 * hour;
        let (from, to) = windows(&request(None, None), six_hours, NOW).unwrap();

        assert_eq!((to - from) / six_hours, 4);
    }

    #[test]
    fn a_short_window_holds_at_most_seventy_two() {
        let five_minutes = 300_000_000;
        let (from, to) = windows(&request(None, None), five_minutes, NOW).unwrap();

        assert_eq!((to - from) / five_minutes, 72);
    }

    #[test]
    fn a_range_is_checked() {
        assert!(windows(&request(Some(NOW), Some(NOW)), FIFTEEN_MINUTES, NOW).is_err());
        assert!(
            windows(
                &request(Some(NOW - 400 * FIFTEEN_MINUTES), Some(NOW)),
                FIFTEEN_MINUTES,
                NOW
            )
            .is_err()
        );
        assert!(
            windows(
                &request(Some(NOW - 360 * FIFTEEN_MINUTES), Some(NOW)),
                FIFTEEN_MINUTES,
                NOW
            )
            .is_ok()
        );
    }
}
