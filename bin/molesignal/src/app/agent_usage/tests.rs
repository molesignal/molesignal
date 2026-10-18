// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::atomic::AtomicBool};

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::agent::{
    chat::{ModelCall, ModelCallObserver, Provider},
    usage::{BudgetLevel, Currency},
};

/// 一行用量的键：组织、天、来源、币种。
type RowKey = (String, i64, UsageOrigin, Option<Currency>);

/// 内存里的用量与预算；`fail` 打开时所有读写都报错。
#[derive(Default)]
struct MemoryUsage {
    rows: Mutex<HashMap<RowKey, UsageDelta>>,
    budgets: Mutex<HashMap<String, UsageBudget>>,
    reads: Mutex<usize>,
    fail: AtomicBool,
}

impl MemoryUsage {
    fn failing(&self) -> Result<()> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::unavailable("meta database is down"));
        }

        Ok(())
    }
}

#[async_trait]
impl AgentUsageRepository for MemoryUsage {
    async fn add(
        &self,
        org_id: &Id,
        day: UsageDay,
        origin: UsageOrigin,
        delta: &UsageDelta,
    ) -> Result<()> {
        self.failing()?;

        let mut rows = self.rows.lock();
        let row = rows
            .entry((org_id.0.clone(), day.0, origin, delta.currency))
            .or_default();
        row.calls += delta.calls;
        row.unpriced_calls += delta.unpriced_calls;
        row.prompt_tokens += delta.prompt_tokens;
        row.completion_tokens += delta.completion_tokens;
        row.cost += delta.cost;
        row.currency = delta.currency;

        Ok(())
    }

    async fn list(&self, org_id: &Id, from: UsageDay, to: UsageDay) -> Result<Vec<DailyUsage>> {
        self.failing()?;

        let mut out: Vec<DailyUsage> = self
            .rows
            .lock()
            .iter()
            .filter(|((org, day, _, _), _)| *org == org_id.0 && (from.0..=to.0).contains(day))
            .map(|((_, day, origin, currency), delta)| DailyUsage {
                day: UsageDay(*day),
                origin: *origin,
                calls: delta.calls,
                unpriced_calls: delta.unpriced_calls,
                prompt_tokens: delta.prompt_tokens,
                completion_tokens: delta.completion_tokens,
                cost: delta.cost,
                currency: *currency,
            })
            .collect();

        out.sort_by_key(|row| (row.day, row.origin, row.currency));
        Ok(out)
    }

    async fn tokens_on(&self, org_id: &Id, day: UsageDay) -> Result<i64> {
        self.failing()?;
        *self.reads.lock() += 1;

        Ok(self
            .rows
            .lock()
            .iter()
            .filter(|((org, row_day, _, _), _)| *org == org_id.0 && *row_day == day.0)
            .map(|(_, delta)| delta.tokens())
            .sum())
    }

    async fn get_budget(&self, org_id: &Id) -> Result<Option<UsageBudget>> {
        self.failing()?;
        Ok(self.budgets.lock().get(&org_id.0).cloned())
    }

    async fn put_budget(&self, budget: UsageBudget) -> Result<UsageBudget> {
        self.failing()?;
        self.budgets
            .lock()
            .insert(budget.org_id.0.clone(), budget.clone());
        Ok(budget)
    }
}

fn usage_with(daily_token_budget: u64) -> (Arc<AgentUsage>, Arc<MemoryUsage>, TraceReceiver) {
    let repository = Arc::new(MemoryUsage::default());
    let (queue, receiver) = trace_queue();

    let usage = Arc::new(AgentUsage::new(
        repository.clone(),
        &AgentSettings {
            daily_token_budget,
            budget_warning_percent: 80,
            ..AgentSettings::default()
        },
        queue,
    ));

    (usage, repository, receiver)
}

fn call(prompt_tokens: i64, completion_tokens: i64) -> ModelCall {
    ModelCall {
        provider: Provider::OpenAi,
        model: "gpt-4o".into(),
        prompt_tokens,
        cached_prompt_tokens: 0,
        completion_tokens,
        latency_ms: 1200,
        streaming: true,
        tool_calls: 1,
        finish_reason: Some("tool_calls".into()),
        error: None,
    }
}

fn org() -> Id {
    Id("org-a".into())
}

#[tokio::test]
async fn an_unlimited_instance_never_refuses() {
    let (usage, _, _receiver) = usage_with(0);
    let recorder = usage.recorder(UsageScope::new(org(), UsageOrigin::Chat));

    recorder.observe(call(5_000_000, 5_000_000)).await;

    let gate = usage.gate(&org(), false).await;
    assert!(!gate.refused());
    assert_eq!(gate.status().level, BudgetLevel::Unlimited);
}

#[tokio::test]
async fn recorded_calls_move_the_cached_status_to_the_limit() {
    let (usage, repository, _receiver) = usage_with(1_000);

    let status = usage.status(&org()).await;
    assert_eq!(status.level, BudgetLevel::Ok);
    assert_eq!(*repository.reads.lock(), 1);

    let recorder = usage.recorder(UsageScope::new(org(), UsageOrigin::Inspection));
    recorder.observe(call(600, 250)).await;

    // 缓存没有过期，状况由记账直接推进，不再查库。
    let status = usage.status(&org()).await;
    assert_eq!(status.used_tokens, 850);
    assert_eq!(status.level, BudgetLevel::Warning);
    assert_eq!(*repository.reads.lock(), 1);

    recorder.observe(call(100, 50)).await;
    assert_eq!(usage.status(&org()).await.level, BudgetLevel::Exhausted);
}

#[tokio::test]
async fn an_exhausted_budget_refuses_background_calls_and_follows_the_mode_for_people() {
    let (usage, _, _receiver) = usage_with(100);
    let recorder = usage.recorder(UsageScope::new(org(), UsageOrigin::Chat));

    recorder.observe(call(80, 40)).await;
    usage.statuses.lock().clear();

    assert!(usage.gate(&org(), false).await.refused());
    assert!(usage.gate(&org(), true).await.refused());

    usage
        .put_budget(
            &org(),
            None,
            InteractiveBudgetMode::Warn,
            &Id("admin".into()),
        )
        .await
        .unwrap();

    assert!(usage.gate(&org(), false).await.refused());

    let interactive = usage.gate(&org(), true).await;
    assert!(!interactive.refused());
    assert!(interactive.status().should_warn());
}

#[tokio::test]
async fn an_organization_limit_overrides_the_instance_default() {
    let (usage, _, _receiver) = usage_with(1_000_000);
    let admin = Id("admin".into());

    let settings = usage
        .put_budget(&org(), Some(50), InteractiveBudgetMode::Block, &admin)
        .await
        .unwrap();

    assert_eq!(settings.org_limit, Some(50));
    assert_eq!(settings.instance_default, 1_000_000);
    assert_eq!(settings.effective_limit, 50);
    assert_eq!(settings.updated_by, Some(admin.clone()));

    usage
        .recorder(UsageScope::new(org(), UsageOrigin::Rca))
        .observe(call(40, 20))
        .await;

    assert!(usage.gate(&org(), false).await.refused());

    // 0 表示这个组织不限，即使实例有默认上限。
    usage
        .put_budget(&org(), Some(0), InteractiveBudgetMode::Block, &admin)
        .await
        .unwrap();

    assert_eq!(usage.status(&org()).await.level, BudgetLevel::Unlimited);

    assert!(
        usage
            .put_budget(&org(), Some(-1), InteractiveBudgetMode::Block, &admin)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_recorder_adds_up_tokens_and_prices_only_when_set() {
    let (usage, repository, mut receiver) = usage_with(0);

    let unpriced = usage.recorder(UsageScope::new(org(), UsageOrigin::Chat));
    unpriced.observe(call(1_000, 500)).await;
    assert_eq!(unpriced.cost(), None);

    let mut scope = UsageScope::new(org(), UsageOrigin::Chat);
    scope.prices = ModelPrices::from_parts(Currency::USD, Some(2.0), None, Some(8.0));
    scope.chat_id = Some(Id("chat-1".into()));
    scope.user_id = Some(Id("user-1".into()));

    let priced = usage.recorder(scope);
    priced.observe(call(1_000_000, 0)).await;
    priced.observe(call(0, 500_000)).await;

    let totals = priced.totals();
    assert_eq!(totals.calls, 2);
    assert_eq!(totals.unpriced_calls, 0);
    assert_eq!(totals.tokens(), 1_500_000);
    let cost = priced.cost().unwrap();
    assert!((cost.amount - 6.0).abs() < 1e-9);
    assert_eq!(cost.currency, Currency::USD);
    assert_eq!(unpriced.totals().unpriced_calls, 1);

    // 没有单价的那次调用记在没有币种的那行，当天的成本里没有它。
    let rows = repository
        .list(&org(), UsageDay::today(), UsageDay::today())
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].currency, rows[0].calls, rows[0].unpriced_calls),
        (None, 1, 1)
    );
    assert_eq!(rows[0].tokens(), 1_500);
    assert_eq!((rows[1].currency, rows[1].calls), (Some(Currency::USD), 2));
    assert!((rows[1].cost - 6.0).abs() < 1e-9);

    let first = receiver.recv().await.unwrap();
    assert!(first.fields.get("cost").is_none());
    assert!(first.fields.get("cost_currency").is_none());
    assert_eq!(first.fields["origin"], json!("chat"));
    assert_eq!(first.fields["total_tokens"], json!(1_500));
    assert_eq!(first.fields["latency_ms"], json!(1200));
    assert_eq!(first.fields["status"], json!("ok"));

    let second = receiver.recv().await.unwrap();
    assert_eq!(second.fields["cost"], json!(2.0));
    assert_eq!(second.fields["cost_currency"], json!("USD"));
    assert_eq!(second.fields["chat_id"], json!("chat-1"));
    assert_eq!(second.fields["user_id"], json!("user-1"));
    assert_eq!(second.org_id, org());
}

#[tokio::test]
async fn cached_input_is_charged_at_the_cache_price_in_the_providers_currency() {
    let (usage, repository, mut receiver) = usage_with(0);
    let cny = Currency::parse("CNY").unwrap();

    let mut scope = UsageScope::new(org(), UsageOrigin::Chat);
    scope.prices = ModelPrices::from_parts(cny, Some(2.0), Some(0.5), Some(8.0));
    let recorder = usage.recorder(scope);

    let mut cached = call(1_000_000, 0);
    cached.cached_prompt_tokens = 800_000;
    recorder.observe(cached).await;

    // 20 万未命中 × 2 + 80 万命中 × 0.5。
    let cost = recorder.cost().unwrap();
    assert!((cost.amount - 0.8).abs() < 1e-9, "{}", cost.amount);
    assert_eq!(cost.currency, cny);

    let rows = repository
        .list(&org(), UsageDay::today(), UsageDay::today())
        .await
        .unwrap();
    assert_eq!(rows[0].currency, Some(cny));

    let traced = receiver.recv().await.unwrap();
    assert_eq!(traced.fields["cached_prompt_tokens"], json!(800_000));
    assert_eq!(traced.fields["cost_currency"], json!("CNY"));
}

#[tokio::test]
async fn failed_calls_are_traced_with_a_short_error() {
    let (usage, _, mut receiver) = usage_with(0);
    let recorder = usage.recorder(UsageScope::new(org(), UsageOrigin::LlmEval));

    let mut failed = call(0, 0);
    failed.error = Some("x".repeat(2_000));
    recorder.observe(failed).await;

    let event = receiver.recv().await.unwrap();
    assert_eq!(event.fields["status"], json!("error"));
    assert_eq!(event.fields["error"].as_str().unwrap().chars().count(), 500);
    assert_eq!(event.fields["origin"], json!("llm_eval"));
}

#[tokio::test]
async fn an_unreadable_usage_store_lets_calls_through() {
    let (usage, repository, _receiver) = usage_with(10);
    repository
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let gate = usage.gate(&org(), false).await;
    assert!(!gate.refused());

    // 记不上也不影响调用方。
    usage
        .recorder(UsageScope::new(org(), UsageOrigin::Chat))
        .observe(call(10, 10))
        .await;
}

#[tokio::test]
async fn history_covers_the_requested_days_up_to_today() {
    let (usage, repository, _receiver) = usage_with(0);
    let today = UsageDay::today();

    for (offset, tokens) in [(0, 10), (3, 20), (120, 30)] {
        repository
            .add(
                &org(),
                today.minus(offset),
                UsageOrigin::Chat,
                &UsageDelta {
                    calls: 1,
                    unpriced_calls: 1,
                    prompt_tokens: tokens,
                    completion_tokens: 0,
                    cost: 0.0,
                    currency: None,
                },
            )
            .await
            .unwrap();
    }

    let week = usage.history(&org(), 7).await.unwrap();
    assert_eq!(
        week.iter().map(|row| row.prompt_tokens).collect::<Vec<_>>(),
        vec![20, 10]
    );

    // 回看天数有上限，120 天前的那一天不在里面。
    let capped = usage.history(&org(), 10_000).await.unwrap();
    assert_eq!(capped.len(), 2);
}
