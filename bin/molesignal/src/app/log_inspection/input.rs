// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 保存一条巡检时请求里说的内容，以及不依赖外部状态的那部分校验。
//!
//! 数据流、模型配置、通知连接器存不存在要去问别处，在 [`super::service`] 里核对。

use chrono_tz::Tz;

use super::{InspectionLimits, prompt, schedule};
use crate::{
    domain::log_inspection::{InspectionNotifyRoute, InspectionSchedule, NotifyWhen},
    shared::{Error, Result, ids::Id},
};

const NAME_CHARS: usize = 120;
const STREAM_CHARS: usize = 255;
const STATEMENT_CHARS: usize = 8_000;
/// 与页面上「立即巡检」的巡检要求同长。
const REQUIREMENTS_CHARS: usize = 2_000;
const MODEL_CHARS: usize = 255;
const TARGET_CHARS: usize = 512;
/// 一次巡检至少看五分钟，至多看一周。
const MIN_WINDOW_SECS: i64 = 5 * 60;
const MAX_WINDOW_SECS: i64 = 7 * 24 * 3600;
const MAX_ROUTES: usize = 10;

#[derive(Debug, Clone)]
pub struct InspectionInput {
    pub name: String,
    pub stream: String,
    pub statement: String,
    pub window_secs: i64,
    pub requirements: String,
    pub schedule: InspectionSchedule,
    pub timezone: String,
    pub provider_id: String,
    pub model: Option<String>,
    pub notify_when: NotifyWhen,
    pub notify_routes: Vec<InspectionNotifyRoute>,
    pub language: String,
    pub enabled: bool,
}

/// 校验过、收拾过的输入：首尾空白去掉，时区已解析。
#[derive(Debug, Clone)]
pub(super) struct ValidInput {
    pub name: String,
    pub stream: String,
    pub statement: String,
    pub window_secs: i64,
    pub requirements: String,
    pub schedule: InspectionSchedule,
    pub timezone: Tz,
    pub provider_id: Id,
    pub model: Option<String>,
    pub notify_when: NotifyWhen,
    pub notify_routes: Vec<InspectionNotifyRoute>,
    pub language: String,
    pub enabled: bool,
}

pub(super) fn validate(input: InspectionInput, limits: &InspectionLimits) -> Result<ValidInput> {
    let name = bounded("name", input.name.trim(), 1, NAME_CHARS)?;
    let stream = bounded("stream", input.stream.trim(), 1, STREAM_CHARS)?;
    let statement = bounded("statement", input.statement.trim(), 0, STATEMENT_CHARS)?;

    let requirements = bounded(
        "requirements",
        input.requirements.trim(),
        0,
        REQUIREMENTS_CHARS,
    )?;

    if !(MIN_WINDOW_SECS..=MAX_WINDOW_SECS).contains(&input.window_secs) {
        return Err(Error::invalid(format!(
            "window_secs must be between {MIN_WINDOW_SECS} and {MAX_WINDOW_SECS}"
        )));
    }

    schedule::validate(&input.schedule, limits.min_interval_secs)?;
    let timezone = schedule::parse_timezone(&input.timezone)?;
    let provider_id = bounded("provider_id", input.provider_id.trim(), 1, 64)?;

    let model = match input.model.as_deref().map(str::trim) {
        Some(model) if !model.is_empty() => Some(bounded("model", model, 1, MODEL_CHARS)?),
        _ => None,
    };

    let language = input.language.trim().to_ascii_lowercase();

    if prompt::language_name(&language).is_none() {
        return Err(Error::invalid("language must be en-us or zh-cn"));
    }

    Ok(ValidInput {
        name,
        stream,
        statement,
        window_secs: input.window_secs,
        requirements,
        schedule: input.schedule,
        timezone,
        provider_id: Id::from_string(provider_id),
        model,
        notify_when: input.notify_when,
        notify_routes: routes(input.notify_routes)?,
        language,
        enabled: input.enabled,
    })
}

/// 通知渠道：数量有上限，目标不为空，同一个连接器上的同一个目标只出现一次。
fn routes(routes: Vec<InspectionNotifyRoute>) -> Result<Vec<InspectionNotifyRoute>> {
    if routes.len() > MAX_ROUTES {
        return Err(Error::invalid(format!(
            "a log inspection notifies at most {MAX_ROUTES} channels"
        )));
    }

    let mut kept: Vec<InspectionNotifyRoute> = Vec::with_capacity(routes.len());

    for route in routes {
        let route = InspectionNotifyRoute {
            target: bounded("notify_routes.target", route.target.trim(), 1, TARGET_CHARS)?,
            ..route
        };

        if kept.contains(&route) {
            return Err(Error::invalid("notify_routes must not repeat a channel"));
        }

        kept.push(route);
    }

    Ok(kept)
}

/// 字符数在 `min..=max` 之内的文本。
fn bounded(field: &str, value: &str, min: usize, max: usize) -> Result<String> {
    let chars = value.chars().count();

    if chars < min {
        return Err(Error::invalid(format!("{field} is required")));
    }

    if chars > max {
        return Err(Error::invalid(format!(
            "{field} must be at most {max} characters"
        )));
    }

    Ok(value.to_string())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::domain::notify::connector::NotifyTargetType;

    pub(in crate::app::log_inspection) fn limits() -> InspectionLimits {
        InspectionLimits::new(3600, 20, 2, 300, 30)
    }

    pub(in crate::app::log_inspection) fn input() -> InspectionInput {
        InspectionInput {
            name: "  payments  ".into(),
            stream: "app_logs".into(),
            statement: " level = 'ERROR' ".into(),
            window_secs: 3600,
            requirements: String::new(),
            schedule: InspectionSchedule::Interval { every_minutes: 60 },
            timezone: "Asia/Shanghai".into(),
            provider_id: "provider-1".into(),
            model: Some("  ".into()),
            notify_when: NotifyWhen::Abnormal,
            notify_routes: vec![route("oc_ops ")],
            language: "ZH-CN".into(),
            enabled: true,
        }
    }

    fn route(target: &str) -> InspectionNotifyRoute {
        InspectionNotifyRoute {
            connector_id: Id::from_string("connector-1"),
            target_type: NotifyTargetType::FixedGroup,
            target: target.into(),
        }
    }

    #[test]
    fn a_valid_input_is_tidied() {
        let valid = validate(input(), &limits()).unwrap();

        assert_eq!(valid.name, "payments");
        assert_eq!(valid.statement, "level = 'ERROR'");
        assert_eq!(valid.model, None);
        assert_eq!(valid.language, "zh-cn");
        assert_eq!(valid.notify_routes[0].target, "oc_ops");
        assert_eq!(valid.timezone.name(), "Asia/Shanghai");
    }

    #[test]
    fn what_cannot_be_inspected_or_scheduled_is_refused() {
        type Change = fn(&mut InspectionInput);

        let cases: [(&str, Change); 9] = [
            ("no name", |input| input.name = "   ".into()),
            ("long name", |input| input.name = "x".repeat(121)),
            ("no stream", |input| input.stream = String::new()),
            ("short window", |input| input.window_secs = 60),
            ("long window", |input| input.window_secs = 8 * 24 * 3600),
            ("unknown time zone", |input| input.timezone = "Local".into()),
            ("no model configuration", |input| {
                input.provider_id = " ".into()
            }),
            ("unsupported language", |input| {
                input.language = "fr-fr".into()
            }),
            ("too often", |input| {
                input.schedule = InspectionSchedule::Interval { every_minutes: 15 }
            }),
        ];

        for (case, change) in cases {
            let mut input = input();
            change(&mut input);
            assert!(validate(input, &limits()).is_err(), "{case}");
        }
    }

    #[test]
    fn a_channel_is_named_once_and_never_blank() {
        let mut repeated = input();
        repeated.notify_routes = vec![route("oc_ops"), route(" oc_ops ")];
        assert!(validate(repeated, &limits()).is_err());

        let mut blank = input();
        blank.notify_routes = vec![route("  ")];
        assert!(validate(blank, &limits()).is_err());

        let mut many = input();
        many.notify_routes = (0..11).map(|n| route(&format!("group-{n}"))).collect();
        assert!(validate(many, &limits()).is_err());

        // 不通知任何人也是一种选择：报告照样留在页面上。
        let mut silent = input();
        silent.notify_routes.clear();
        assert!(validate(silent, &limits()).is_ok());
    }
}
