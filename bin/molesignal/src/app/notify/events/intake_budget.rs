// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tells people when a source nears or passes its intake budget. Only that: intake goes on as it
//! did. A level is told once for a period and a limit, however many nodes look and however often,
//! because the event's id says which; raising the limit makes a later pass news again.

use std::{collections::BTreeMap, sync::Arc};

use serde_json::json;

use crate::{
    app::{
        intake::usage::{BudgetStanding, IntakeSourceDirectory, SourceLabel, budget_standings},
        notify::{NotifyDispatch, NotifyEngine},
    },
    domain::{
        i18n_language::{I18nLanguageRepository, LanguagePack},
        intake::usage::{
            budget::{BudgetState, IntakeBudget, IntakeBudgetRepository},
            source::{IntakeUsageRepository, UsageDimension},
        },
        notify::{connector::NotifyMessage, policy::NotifyEvent},
    },
    infra::persistence::repositories::workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub const INTAKE_BUDGET_WARNING_EVENT: &str = "intake.budget.warning";
pub const INTAKE_BUDGET_EXCEEDED_EVENT: &str = "intake.budget.exceeded";

const LANGUAGE_KEY_PREFIX: &str = "intake_budget.";

pub struct IntakeBudgetEventProducer {
    engine: Arc<NotifyEngine>,
    budgets: Arc<dyn IntakeBudgetRepository>,
    usage: Arc<dyn IntakeUsageRepository>,
    sources: Arc<IntakeSourceDirectory>,
    language: Arc<dyn I18nLanguageRepository>,
    workspace: Arc<dyn WorkspacePreferenceDefaultsRepository>,
}

impl IntakeBudgetEventProducer {
    pub fn new(
        engine: Arc<NotifyEngine>,
        budgets: Arc<dyn IntakeBudgetRepository>,
        usage: Arc<dyn IntakeUsageRepository>,
        sources: Arc<IntakeSourceDirectory>,
        language: Arc<dyn I18nLanguageRepository>,
        workspace: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    ) -> Self {
        Self {
            engine,
            budgets,
            usage,
            sources,
            language,
            workspace,
        }
    }

    /// Looks at the budgets of one organization and queues what is news. The notify engine delivers
    /// it on its own round.
    pub async fn tick(&self, org_id: &Id, now: TimestampMicros) -> Result<()> {
        let budgets = self.budgets.list(org_id).await?;

        if budgets.is_empty() {
            return Ok(());
        }

        let standings = budget_standings(self.usage.as_ref(), org_id, &budgets, now).await?;

        let due = budgets
            .iter()
            .zip(&standings)
            .filter(|(_, standing)| {
                matches!(
                    standing.state,
                    Some(BudgetState::Warning | BudgetState::Exceeded)
                )
            })
            .collect::<Vec<_>>();

        if due.is_empty() {
            return Ok(());
        }

        let workspace = self.workspace.get(org_id).await?;

        let copy = self
            .language
            .load_pack(LANGUAGE_KEY_PREFIX, &workspace.language)
            .await?;

        for dimension in UsageDimension::ALL {
            let keys = due
                .iter()
                .filter(|(budget, _)| budget.dimension == dimension)
                .map(|(budget, _)| budget.source_key.as_str())
                .collect::<Vec<_>>();

            if keys.is_empty() {
                continue;
            }

            let book = self.sources.book(org_id, dimension, &keys).await;

            for (budget, standing) in due
                .iter()
                .filter(|(budget, _)| budget.dimension == dimension)
            {
                let Some(label) = book.labels.get(&budget.source_key) else {
                    continue;
                };

                let dispatch = match budget_dispatch(budget, standing, label, &copy, now) {
                    Ok(Some(dispatch)) => dispatch,
                    Ok(None) => continue,
                    Err(error) => {
                        tracing::warn!(
                            org_id = %org_id,
                            budget_id = %budget.id,
                            %error,
                            "intake budget notification could not be written"
                        );

                        continue;
                    }
                };

                if let Err(error) = self.engine.enqueue_event(dispatch).await {
                    tracing::warn!(
                        org_id = %org_id,
                        budget_id = %budget.id,
                        %error,
                        "intake budget notify event enqueue failed"
                    );
                }
            }
        }

        Ok(())
    }
}

/// The notification of where a budget stands; none while it is within bounds or not known.
pub(in crate::app::notify) fn budget_dispatch(
    budget: &IntakeBudget,
    standing: &BudgetStanding,
    label: &SourceLabel,
    copy: &LanguagePack,
    now: TimestampMicros,
) -> Result<Option<NotifyDispatch>> {
    let Some(used_bytes) = standing.used_bytes else {
        return Ok(None);
    };

    let (event_type, level, event_id) = match standing.state {
        Some(BudgetState::Warning) => (
            INTAKE_BUDGET_WARNING_EVENT,
            "warning",
            format!(
                "{INTAKE_BUDGET_WARNING_EVENT}:{}:{}:{}:{}",
                budget.id, standing.period_start, budget.limit_size, budget.warn_percent
            ),
        ),
        Some(BudgetState::Exceeded) => (
            INTAKE_BUDGET_EXCEEDED_EVENT,
            "exceeded",
            format!(
                "{INTAKE_BUDGET_EXCEEDED_EVENT}:{}:{}:{}",
                budget.id, standing.period_start, budget.limit_size
            ),
        ),
        _ => return Ok(None),
    };

    let source = copy.render(
        &format!("{LANGUAGE_KEY_PREFIX}source.{}", label.kind),
        &[("name", &label.name)],
    )?;

    let period = copy.render(
        &format!("{LANGUAGE_KEY_PREFIX}period.{}", budget.period.as_str()),
        &[],
    )?;

    let used = readable_size(used_bytes);
    let limit = readable_size(budget.limit_bytes());
    let percent = (budget.used_ratio(used_bytes) * 100.0).floor() as u64;
    let percent_text = percent.to_string();

    let title = copy.render(
        &format!("{LANGUAGE_KEY_PREFIX}{level}.title"),
        &[("source", &source)],
    )?;

    let text = copy.render(
        &format!("{LANGUAGE_KEY_PREFIX}{level}.text"),
        &[
            ("source", &source),
            ("used", &used),
            ("limit", &limit),
            ("period", &period),
            ("percent", &percent_text),
        ],
    )?;

    let attributes = json!({
        "user_ids": budget
            .notify_user_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "budget_id": budget.id.to_string(),
        "dimension": budget.dimension.as_str(),
        "source_key": budget.source_key,
        "source_kind": label.kind,
        "source_name": label.name,
        "period": budget.period.as_str(),
        "period_start": standing.period_start,
        "limit_size": budget.limit_size,
        "warn_percent": budget.warn_percent,
        "used_bytes": used_bytes,
        "used_percent": percent,
    });

    let mut metadata = BTreeMap::new();
    metadata.insert("budget_id".into(), budget.id.to_string());
    metadata.insert("event_type".into(), event_type.into());

    Ok(Some(NotifyDispatch {
        event: NotifyEvent {
            id: event_id,
            event_type: event_type.into(),
            organization_id: budget.org_id.clone(),
            occurred_at: now,
            attributes,
        },
        message: NotifyMessage {
            title: title.clone(),
            text: text.clone(),
            markdown: Some(format!("**{title}**\n\n{text}")),
            html: None,
            metadata,
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
    }))
}

/// Bytes as a person reads them, in the units of the pages: `1.5 GiB`.
fn readable_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{} {}", (value * 100.0).round() / 100.0, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::intake::usage::budget::BudgetPeriod, shared::i18n_language_seed::seeded_pack,
    };

    const MIB: u64 = 1024 * 1024;

    fn budget(limit_size: u64) -> IntakeBudget {
        IntakeBudget {
            id: Id::from_string("b-1"),
            org_id: Id::from_string("org-1"),
            dimension: UsageDimension::Credential,
            source_key: "api_token:t-1".into(),
            period: BudgetPeriod::Day,
            limit_size,
            warn_percent: 80,
            notify_user_ids: vec![Id::from_string("u-1"), Id::from_string("u-2")],
            created_by: Id::from_string("u-1"),
            updated_by: Id::from_string("u-1"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn standing(budget: &IntakeBudget, used_bytes: Option<u64>) -> BudgetStanding {
        BudgetStanding {
            period_start: 86_400_000_000,
            used_bytes,
            used_complete: true,
            used_ratio: used_bytes.map(|bytes| budget.used_ratio(bytes)),
            state: used_bytes.map(|bytes| budget.state_at(bytes)),
        }
    }

    fn label() -> SourceLabel {
        SourceLabel {
            kind: "api_token",
            subkind: Some("personal".into()),
            name: "checkout-prod".into(),
            detail: None,
            missing: false,
        }
    }

    #[test]
    fn nothing_is_told_within_bounds_or_when_not_known() {
        let budget = budget(100);
        let copy = seeded_pack(LANGUAGE_KEY_PREFIX, "en-us");

        for used in [None, Some(0), Some(79 * MIB)] {
            let told = budget_dispatch(
                &budget,
                &standing(&budget, used),
                &label(),
                &copy,
                TimestampMicros(1),
            )
            .unwrap();

            assert!(told.is_none(), "{used:?}");
        }
    }

    #[test]
    fn warning_then_exceeded_are_two_events_of_one_period() {
        let budget = budget(100);
        let copy = seeded_pack(LANGUAGE_KEY_PREFIX, "en-us");

        let warning = budget_dispatch(
            &budget,
            &standing(&budget, Some(80 * MIB)),
            &label(),
            &copy,
            TimestampMicros(1),
        )
        .unwrap()
        .unwrap();

        assert_eq!(warning.event.event_type, INTAKE_BUDGET_WARNING_EVENT);
        assert_eq!(
            warning.event.id,
            "intake.budget.warning:b-1:86400000000:100:80"
        );
        assert_eq!(
            warning.message.title,
            "Intake budget nearly used: API token checkout-prod"
        );
        assert_eq!(
            warning.message.text,
            "API token checkout-prod has taken in 80 MiB of its 100 MiB budget for today (UTC) \
             (80%). Data keeps coming in; nothing is dropped."
        );
        assert_eq!(warning.event.attributes["user_ids"], json!(["u-1", "u-2"]));

        let exceeded = budget_dispatch(
            &budget,
            &standing(&budget, Some(150 * MIB)),
            &label(),
            &copy,
            TimestampMicros(1),
        )
        .unwrap()
        .unwrap();

        assert_eq!(exceeded.event.event_type, INTAKE_BUDGET_EXCEEDED_EVENT);
        assert_eq!(
            exceeded.event.id,
            "intake.budget.exceeded:b-1:86400000000:100"
        );
        assert_eq!(exceeded.event.attributes["used_percent"], json!(150));
    }

    #[test]
    fn a_higher_limit_is_a_new_event_and_a_new_day_too() {
        let copy = seeded_pack(LANGUAGE_KEY_PREFIX, "en-us");
        let small = budget(100);
        let large = budget(200);
        let used = Some(500 * MIB);

        let first = budget_dispatch(
            &small,
            &standing(&small, used),
            &label(),
            &copy,
            TimestampMicros(1),
        )
        .unwrap()
        .unwrap();

        let raised = budget_dispatch(
            &large,
            &standing(&large, used),
            &label(),
            &copy,
            TimestampMicros(1),
        )
        .unwrap()
        .unwrap();

        let mut next_day = standing(&small, used);
        next_day.period_start += 86_400_000_000;

        let tomorrow = budget_dispatch(&small, &next_day, &label(), &copy, TimestampMicros(1))
            .unwrap()
            .unwrap();

        assert_ne!(first.event.id, raised.event.id);
        assert_ne!(first.event.id, tomorrow.event.id);
    }

    #[test]
    fn the_text_is_in_the_language_of_the_workspace() {
        let budget = budget(1024);
        let copy = seeded_pack(LANGUAGE_KEY_PREFIX, "zh-cn");

        let told = budget_dispatch(
            &budget,
            &standing(&budget, Some(2048 * MIB)),
            &label(),
            &copy,
            TimestampMicros(1),
        )
        .unwrap()
        .unwrap();

        assert_eq!(told.message.title, "接入超出预算：API Token checkout-prod");
        assert_eq!(
            told.message.text,
            "API Token checkout-prod 今天（UTC）已接入 2 GiB，超出预算 1 GiB（已用 200%）。数据照常接入，不会丢弃。"
        );
    }

    #[test]
    fn every_source_a_budget_can_be_set_on_has_its_words() {
        for language in ["en-us", "zh-cn"] {
            let copy = seeded_pack(LANGUAGE_KEY_PREFIX, language);

            for kind in ["api_token", "connector", "user", "stream", "service"] {
                assert!(
                    copy.get(&format!("{LANGUAGE_KEY_PREFIX}source.{kind}"))
                        .is_some(),
                    "{language} {kind}"
                );
            }

            for period in BudgetPeriod::ALL {
                assert!(
                    copy.get(&format!("{LANGUAGE_KEY_PREFIX}period.{}", period.as_str()))
                        .is_some(),
                    "{language} {period:?}"
                );
            }
        }
    }

    #[test]
    fn sizes_read_in_the_units_of_the_pages() {
        assert_eq!(readable_size(0), "0 B");
        assert_eq!(readable_size(1023), "1023 B");
        assert_eq!(readable_size(1536), "1.5 KiB");
        assert_eq!(readable_size(5 * MIB), "5 MiB");
        assert_eq!(readable_size(1024 * 1024 * MIB), "1 TiB");
    }
}
