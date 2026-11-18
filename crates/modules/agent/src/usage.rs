// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent usage: daily token counts, daily organization budgets, and model unit prices.
//!
//! Usage is counted by UTC calendar day and call origin. Budgets consider only tokens: this
//! repository has no model price list, so costs can only be calculated after an administrator sets
//! unit prices on the model service. Costs use the currency selected for that model service, with
//! separate totals for each currency. Once the budget is exhausted, all background calls (automatic
//! RCA, inspections, and LLM functions in write pipelines) are skipped; calls with a user waiting
//! (chat and manual RCA generation) are blocked or only show a warning, according to the
//! organization settings.

use std::fmt;

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Default budget warning threshold: warnings start at this percentage of the limit.
pub const DEFAULT_WARNING_PERCENT: u8 = 80;

/// The purpose of a model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageOrigin {
    /// Chat, including dashboard creation.
    Chat,
    /// Root cause analysis of incidents, including automatic background and manual generation.
    Rca,
    /// Scheduled log inspections.
    Inspection,
    /// LLM functions in write pipelines (`_llm_eval`).
    LlmEval,
    /// Automation runs, including those triggered by events.
    Automation,
    /// Review scoring and regenerated answers in promotion checks.
    Evaluation,
    /// Summaries and chapters for RUM session replays.
    ReplaySummary,
}

impl UsageOrigin {
    pub const ALL: [Self; 7] = [
        Self::Chat,
        Self::Rca,
        Self::Inspection,
        Self::LlmEval,
        Self::Automation,
        Self::Evaluation,
        Self::ReplaySummary,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Rca => "rca",
            Self::Inspection => "inspection",
            Self::LlmEval => "llm_eval",
            Self::Automation => "automation",
            Self::Evaluation => "evaluation",
            Self::ReplaySummary => "replay_summary",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|origin| origin.as_str() == value)
            .ok_or_else(|| Error::invalid(format!("unknown usage origin `{value}`")))
    }
}

/// A UTC day, stored as the number of days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UsageDay(pub i64);

impl UsageDay {
    pub fn of(at: TimestampMicros) -> Self {
        Self(at.0.div_euclid(MICROS_PER_DAY))
    }

    pub fn today() -> Self {
        Self::of(TimestampMicros::now())
    }

    /// The start of this day (midnight UTC).
    pub fn start(self) -> TimestampMicros {
        TimestampMicros(self.0.saturating_mul(MICROS_PER_DAY))
    }

    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }

    pub fn minus(self, days: i64) -> Self {
        Self(self.0 - days)
    }
}

/// The amount to add to the counters for one or more calls.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageDelta {
    pub calls: i64,
    /// Calls among these whose model service has no unit prices: they are excluded from costs.
    pub unpriced_calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// Cost in `currency`; 0 when there is no currency.
    pub cost: f64,
    /// Cost currency selected on the model service; absent when all calls lack unit prices.
    pub currency: Option<Currency>,
}

impl UsageDelta {
    pub fn tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// An organization's usage for a day, origin, and currency.
/// Calls without unit prices are recorded in the row with no currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub day: UsageDay,
    pub origin: UsageOrigin,
    pub calls: i64,
    /// Number of calls with no model service unit prices that are excluded from costs.
    pub unpriced_calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: f64,
    pub currency: Option<Currency>,
}

impl DailyUsage {
    pub fn tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// How to handle calls with a user waiting when the budget is exhausted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveBudgetMode {
    /// Stop calling the model and tell the user that today's quota is exhausted.
    #[default]
    Block,
    /// Answer as usual and only warn that the budget is exceeded.
    Warn,
}

impl InteractiveBudgetMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Warn => "warn",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "block" => Ok(Self::Block),
            "warn" => Ok(Self::Warn),
            other => Err(Error::invalid(format!(
                "interactive budget mode must be `block` or `warn`, got `{other}`"
            ))),
        }
    }
}

/// An organization's own budget settings.
/// When this row is absent, all settings follow the instance defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBudget {
    pub org_id: Id,
    /// Daily token limit; `None` follows the instance default, and 0 means unlimited.
    pub daily_token_limit: Option<i64>,
    pub interactive_mode: InteractiveBudgetMode,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLevel {
    /// No limit is set.
    Unlimited,
    Ok,
    /// The warning threshold is reached.
    Warning,
    /// The budget is exhausted.
    Exhausted,
}

/// An organization's budget status for today.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub level: BudgetLevel,
    pub used_tokens: i64,
    /// 0 means unlimited.
    pub limit_tokens: i64,
    pub warning_percent: u8,
    pub interactive_mode: InteractiveBudgetMode,
    /// The time when the counters reset to zero (the next midnight UTC).
    pub resets_at: TimestampMicros,
}

impl BudgetStatus {
    pub fn evaluate(
        used_tokens: i64,
        limit_tokens: i64,
        warning_percent: u8,
        interactive_mode: InteractiveBudgetMode,
        day: UsageDay,
    ) -> Self {
        let warning_percent = warning_percent.clamp(1, 100);

        let level = if limit_tokens <= 0 {
            BudgetLevel::Unlimited
        } else if used_tokens >= limit_tokens {
            BudgetLevel::Exhausted
        } else if i128::from(used_tokens) * 100
            >= i128::from(limit_tokens) * i128::from(warning_percent)
        {
            BudgetLevel::Warning
        } else {
            BudgetLevel::Ok
        };

        Self {
            level,
            used_tokens,
            limit_tokens: limit_tokens.max(0),
            warning_percent,
            interactive_mode,
            resets_at: day.next().start(),
        }
    }

    /// Whether this call is allowed.
    /// Background calls stop when the quota is exhausted; calls with a user waiting proceed in
    /// warning-only mode.
    pub fn allows(&self, interactive: bool) -> bool {
        match self.level {
            BudgetLevel::Exhausted => {
                interactive && self.interactive_mode == InteractiveBudgetMode::Warn
            }
            BudgetLevel::Unlimited | BudgetLevel::Ok | BudgetLevel::Warning => true,
        }
    }

    /// Whether to warn the user: usage is near or already above the limit.
    pub fn should_warn(&self) -> bool {
        matches!(self.level, BudgetLevel::Warning | BudgetLevel::Exhausted)
    }

    /// Count a call toward usage (keeping cached status roughly accurate until the next refresh).
    pub fn with_spent(self, tokens: i64) -> Self {
        Self::evaluate(
            self.used_tokens.saturating_add(tokens.max(0)),
            self.limit_tokens,
            self.warning_percent,
            self.interactive_mode,
            UsageDay::of(TimestampMicros(self.resets_at.0 - 1)),
        )
    }
}

/// Currency: three uppercase ISO 4217 letters, such as `USD`, `CNY`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

impl Currency {
    /// Model services with no currency selected use US dollars.
    pub const USD: Self = Self(*b"USD");

    pub fn parse(code: &str) -> Result<Self> {
        let bytes = code.as_bytes();

        match bytes {
            [a, b, c] if bytes.iter().all(u8::is_ascii_uppercase) => Ok(Self([*a, *b, *c])),
            _ => Err(Error::invalid(format!(
                "currency must be an ISO 4217 code of three capital letters, got `{code}`"
            ))),
        }
    }

    pub fn as_str(&self) -> &str {
        // Only `parse` can construct this: three uppercase ASCII letters.
        std::str::from_utf8(&self.0).unwrap_or("USD")
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Currency {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Self::parse(&code).map_err(serde::de::Error::custom)
    }
}

/// A cost: an amount and its currency.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: Currency,
}

impl Cost {
    /// A cost stored in two columns (amount, currency) exists only when both columns have values.
    pub fn from_columns(amount: Option<f64>, currency: Option<&str>) -> Result<Option<Self>> {
        match (amount, currency) {
            (Some(amount), Some(code)) if !code.is_empty() => Ok(Some(Self {
                amount,
                currency: Currency::parse(code)?,
            })),
            _ => Ok(None),
        }
    }
}

/// Tokens used by a call, including how many input tokens hit the cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    /// All input, including cache hits.
    pub prompt: i64,
    /// The portion of input that hits the cache.
    pub cached_prompt: i64,
    pub completion: i64,
}

/// Model service unit prices per million tokens, in `currency`.
/// Some providers price input cache hits and misses separately: `input_per_mtok` is for misses
/// (and is also the input price for providers that do not distinguish them), while
/// `cached_input_per_mtok` is for hits; when no cache-hit price is set, cached input also uses the
/// input price.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrices {
    pub currency: Currency,
    pub input_per_mtok: f64,
    pub cached_input_per_mtok: Option<f64>,
    pub output_per_mtok: f64,
}

impl ModelPrices {
    /// No pricing exists when both input and output prices are unset; if only one is set, the other
    /// is treated as 0.
    pub fn from_parts(
        currency: Currency,
        input_per_mtok: Option<f64>,
        cached_input_per_mtok: Option<f64>,
        output_per_mtok: Option<f64>,
    ) -> Option<Self> {
        if input_per_mtok.is_none() && output_per_mtok.is_none() {
            return None;
        }

        Some(Self {
            currency,
            input_per_mtok: input_per_mtok.unwrap_or(0.0).max(0.0),
            cached_input_per_mtok: cached_input_per_mtok.map(|price| price.max(0.0)),
            output_per_mtok: output_per_mtok.unwrap_or(0.0).max(0.0),
        })
    }

    /// The cost of these tokens, in `currency`.
    pub fn cost(&self, tokens: TokenCounts) -> f64 {
        let cached = tokens.cached_prompt.clamp(0, tokens.prompt.max(0));
        let uncached = tokens.prompt.max(0) - cached;
        let cached_price = self.cached_input_per_mtok.unwrap_or(self.input_per_mtok);

        let input = uncached as f64 * self.input_per_mtok + cached as f64 * cached_price;
        let output = tokens.completion.max(0) as f64 * self.output_per_mtok;

        (input + output) / 1_000_000.0
    }
}

/// Valid unit prices are nonnegative, finite, and not so large that they clearly indicate incorrect
/// units (including prices in Korean won and Japanese yen).
pub fn validate_price(value: Option<f64>, field: &str) -> Result<()> {
    match value {
        Some(price) if !price.is_finite() || !(0.0..=1_000_000.0).contains(&price) => {
            Err(Error::invalid(format!(
                "{field} must be between 0 and 1000000 per million tokens"
            )))
        }
        _ => Ok(()),
    }
}

#[async_trait]
pub trait AgentUsageRepository: Send + Sync {
    /// Add a call to the counters for this origin on that day.
    async fn add(
        &self,
        org_id: &Id,
        day: UsageDay,
        origin: UsageOrigin,
        delta: &UsageDelta,
    ) -> Result<()>;

    /// An organization's usage for `from..=to`, with rows by day and origin, including only rows
    /// with calls.
    async fn list(&self, org_id: &Id, from: UsageDay, to: UsageDay) -> Result<Vec<DailyUsage>>;

    /// The total tokens used by an organization on a given day.
    async fn tokens_on(&self, org_id: &Id, day: UsageDay) -> Result<i64>;

    async fn get_budget(&self, org_id: &Id) -> Result<Option<UsageBudget>>;

    async fn put_budget(&self, budget: UsageBudget) -> Result<UsageBudget>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_round_trip() {
        for origin in UsageOrigin::ALL {
            assert_eq!(UsageOrigin::parse(origin.as_str()).unwrap(), origin);
        }

        assert!(UsageOrigin::parse("replay").is_err());
    }

    #[test]
    fn days_are_utc_calendar_days() {
        let day = UsageDay::of(TimestampMicros(1_760_572_800_000_000 + 3_600_000_000));
        assert_eq!(day.start(), TimestampMicros(1_760_572_800_000_000));
        assert_eq!(day.next().start().0 - day.start().0, MICROS_PER_DAY);
        assert_eq!(UsageDay::of(TimestampMicros(-1)), UsageDay(-1));
    }

    #[test]
    fn budget_levels_follow_the_limit_and_the_warning_threshold() {
        let level = |used, limit| {
            BudgetStatus::evaluate(used, limit, 80, InteractiveBudgetMode::Block, UsageDay(0)).level
        };

        assert_eq!(level(1_000_000, 0), BudgetLevel::Unlimited);
        assert_eq!(level(79, 100), BudgetLevel::Ok);
        assert_eq!(level(80, 100), BudgetLevel::Warning);
        assert_eq!(level(100, 100), BudgetLevel::Exhausted);
        assert_eq!(level(250, 100), BudgetLevel::Exhausted);
    }

    #[test]
    fn exhausted_budgets_stop_background_calls_and_follow_the_mode_for_interactive_ones() {
        let status = |mode| BudgetStatus::evaluate(100, 100, 80, mode, UsageDay(0));

        let blocking = status(InteractiveBudgetMode::Block);
        assert!(!blocking.allows(false));
        assert!(!blocking.allows(true));
        assert!(blocking.should_warn());

        let warning = status(InteractiveBudgetMode::Warn);
        assert!(!warning.allows(false));
        assert!(warning.allows(true));

        let unlimited =
            BudgetStatus::evaluate(10, 0, 80, InteractiveBudgetMode::Block, UsageDay(0));
        assert!(unlimited.allows(false));
        assert!(!unlimited.should_warn());
    }

    #[test]
    fn spending_moves_a_cached_status_forward() {
        let status = BudgetStatus::evaluate(70, 100, 80, InteractiveBudgetMode::Block, UsageDay(3));
        assert_eq!(status.level, BudgetLevel::Ok);

        let spent = status.with_spent(40);
        assert_eq!(spent.used_tokens, 110);
        assert_eq!(spent.level, BudgetLevel::Exhausted);
        assert_eq!(spent.resets_at, UsageDay(4).start());
    }

    #[test]
    fn prices_turn_tokens_into_cost() {
        let usd = Currency::USD;
        assert_eq!(ModelPrices::from_parts(usd, None, Some(0.5), None), None);

        let prices = ModelPrices::from_parts(usd, Some(2.5), None, Some(10.0)).unwrap();
        let tokens = TokenCounts {
            prompt: 1_000_000,
            cached_prompt: 0,
            completion: 500_000,
        };
        let cost = prices.cost(tokens);
        assert!((cost - 7.5).abs() < 1e-9, "{cost}");

        let input_only = ModelPrices::from_parts(usd, Some(1.0), None, None).unwrap();
        assert_eq!(input_only.output_per_mtok, 0.0);
    }

    #[test]
    fn cached_input_is_charged_at_its_own_price() {
        let cny = Currency::parse("CNY").unwrap();
        let prices = ModelPrices::from_parts(cny, Some(2.0), Some(0.5), Some(8.0)).unwrap();

        // Of 1 million input tokens, 400,000 hit the cache: 600,000 * 2 + 400,000 * 0.5,
        // plus 100,000 output tokens * 8.
        let cost = prices.cost(TokenCounts {
            prompt: 1_000_000,
            cached_prompt: 400_000,
            completion: 100_000,
        });
        assert!((cost - (1.2 + 0.2 + 0.8)).abs() < 1e-9, "{cost}");

        // When no cache-hit price is set, cached input uses the input price.
        let flat = ModelPrices::from_parts(cny, Some(2.0), None, Some(8.0)).unwrap();
        let cost = flat.cost(TokenCounts {
            prompt: 1_000_000,
            cached_prompt: 400_000,
            completion: 0,
        });
        assert!((cost - 2.0).abs() < 1e-9, "{cost}");

        // When reported cache hits exceed input tokens, treat all input as cache hits.
        let cost = prices.cost(TokenCounts {
            prompt: 100,
            cached_prompt: 1_000,
            completion: 0,
        });
        assert!((cost - 100.0 * 0.5 / 1_000_000.0).abs() < 1e-12, "{cost}");
    }

    #[test]
    fn currencies_are_three_capital_letters() {
        assert_eq!(Currency::parse("CNY").unwrap().as_str(), "CNY");
        assert!(Currency::parse("cny").is_err());
        assert!(Currency::parse("YUAN").is_err());
        assert!(Currency::parse("").is_err());
        assert_eq!(
            serde_json::to_string(&Currency::parse("EUR").unwrap()).unwrap(),
            "\"EUR\""
        );
        assert!(serde_json::from_str::<Currency>("\"US\"").is_err());
    }

    #[test]
    fn prices_must_be_plausible() {
        assert!(validate_price(None, "input").is_ok());
        assert!(validate_price(Some(0.0), "input").is_ok());
        assert!(validate_price(Some(75.0), "input").is_ok());
        assert!(validate_price(Some(-1.0), "input").is_err());
        assert!(validate_price(Some(f64::NAN), "input").is_err());
        assert!(validate_price(Some(150_000.0), "input").is_ok());
        assert!(validate_price(Some(1e9), "input").is_err());
    }
}
