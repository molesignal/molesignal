// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 内置模板：从模板建规则时预先填好任务提示、触发与输出。名称、说明与任务提示在
//! `i18n_language`（`automation.template.<key>.name|description|prompt`），用读者的语言。
//! 「事件自动分析」就是事件 RCA 的新引擎：它带着工具调查，结论写回事件的 RCA。

use serde::Serialize;

use crate::{
    agent::automations::{AutomationOutputs, AutomationTrigger, IncidentFilter},
    domain::i18n_language::LanguagePack,
    shared::Result,
};

/// `i18n_language` 里模板文案的键前缀。
pub const KEY_PREFIX: &str = "automation.template.";

/// 模板怎么触发。
enum Starts {
    /// 严重级别为错误或严重的新事件。
    SevereIncidents,
    /// 按这个 Cron 表达式，在读者的时区里。
    Schedule(&'static str),
    /// 外部系统（例如 CI/CD）调接口。
    Api,
}

impl Starts {
    fn trigger(&self, timezone: &str) -> AutomationTrigger {
        match self {
            Self::SevereIncidents => AutomationTrigger::Incident(IncidentFilter {
                severities: vec!["error".into(), "critical".into()],
                ..IncidentFilter::default()
            }),
            Self::Schedule(cron) => AutomationTrigger::Schedule {
                cron: (*cron).into(),
                timezone: timezone.into(),
            },
            Self::Api => AutomationTrigger::Api,
        }
    }
}

/// 一个内置模板的形状；文案在语言表里。
struct Template {
    key: &'static str,
    starts: Starts,
    write_rca: bool,
}

const TEMPLATES: [Template; 5] = [
    Template {
        key: "incident_analysis",
        starts: Starts::SevereIncidents,
        write_rca: true,
    },
    Template {
        key: "daily_health",
        starts: Starts::Schedule("0 9 * * 1-5"),
        write_rca: false,
    },
    Template {
        key: "alert_noise_weekly",
        starts: Starts::Schedule("0 10 * * 1"),
        write_rca: false,
    },
    Template {
        key: "on_call_load_weekly",
        starts: Starts::Schedule("30 10 * * 1"),
        write_rca: false,
    },
    Template {
        key: "change_impact",
        starts: Starts::Api,
        write_rca: false,
    },
];

/// 给读者看的模板：文案已经是他的语言，定时的模板按他的时区。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TemplateView {
    pub key: &'static str,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub trigger: AutomationTrigger,
    pub outputs: AutomationOutputs,
}

pub fn render(pack: &LanguagePack, timezone: &str) -> Result<Vec<TemplateView>> {
    TEMPLATES
        .iter()
        .map(|template| {
            let text =
                |part: &str| pack.render(&format!("{KEY_PREFIX}{}.{part}", template.key), &[]);

            Ok(TemplateView {
                key: template.key,
                name: text("name")?,
                description: text("description")?,
                prompt: text("prompt")?,
                trigger: template.starts.trigger(timezone),
                outputs: AutomationOutputs {
                    channels: Vec::new(),
                    write_rca: template.write_rca,
                },
            })
        })
        .collect()
}

/// 建规则时带的模板键是不是内置模板。
pub fn is_known(key: &str) -> bool {
    TEMPLATES.iter().any(|template| template.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::automations::validate, shared::i18n_language_seed::seeded_pack};

    #[test]
    fn every_template_has_its_words_in_both_languages_and_saves_as_it_is() {
        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(KEY_PREFIX, language);
            let views = render(&pack, "Asia/Shanghai").unwrap();

            assert_eq!(views.len(), TEMPLATES.len());

            for view in &views {
                assert!(
                    !view.name.is_empty() && !view.prompt.is_empty(),
                    "{}",
                    view.key
                );
                assert!(validate::prompt(&view.prompt).is_ok(), "{}", view.key);
                assert!(validate::name(&view.name).is_ok(), "{}", view.key);

                let trigger = validate::trigger(&view.trigger).unwrap();
                assert!(
                    validate::outputs(&view.outputs, &trigger).is_ok(),
                    "{}",
                    view.key
                );
            }
        }

        assert!(is_known("incident_analysis"));
        assert!(!is_known("nightly_backup"));
    }
}
