// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 用量：每日 token 计数、组织的每日预算与模型单价。
//!
//! 用量按 UTC 自然日、按调用来源计数。预算只看 token：本仓没有模型价目表，管理员在模型
//! 服务上填了单价才算得出成本，成本用那个模型服务选的币种记，不同币种分开合计。预算用完后，
//! 后台调用（自动 RCA、巡检、写入管道里的 LLM 函数）一律跳过；有人在等的调用（对话、手动
//! 生成 RCA）按组织设置拦截或只提示。

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// 预算提示的默认阈值：用到上限的这个百分比时开始提醒。
pub const DEFAULT_WARNING_PERCENT: u8 = 80;

/// 一次模型调用是为了什么发起的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageOrigin {
    /// 对话，含仪表盘创作。
    Chat,
    /// 事件的根因分析，后台自动生成与手动生成都算。
    Rca,
    /// 定时日志巡检。
    Inspection,
    /// 写入管道里的 LLM 函数（`_llm_eval`）。
    LlmEval,
}

impl UsageOrigin {
    pub const ALL: [Self; 4] = [Self::Chat, Self::Rca, Self::Inspection, Self::LlmEval];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Rca => "rca",
            Self::Inspection => "inspection",
            Self::LlmEval => "llm_eval",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|origin| origin.as_str() == value)
            .ok_or_else(|| Error::invalid(format!("unknown usage origin `{value}`")))
    }
}

/// UTC 的一天，存成从 1970-01-01 起的天数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UsageDay(pub i64);

impl UsageDay {
    pub fn of(at: TimestampMicros) -> Self {
        Self(at.0.div_euclid(MICROS_PER_DAY))
    }

    pub fn today() -> Self {
        Self::of(TimestampMicros::now())
    }

    /// 这一天开始的时刻（UTC 零点）。
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

/// 一次或几次调用要加进计数的量。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageDelta {
    pub calls: i64,
    /// 其中模型服务没有填单价的调用：它们不计入成本。
    pub unpriced_calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// 用 `currency` 记的成本；没有币种时是 0。
    pub cost: f64,
    /// 成本的币种：模型服务上选的；调用都没有单价时为空。
    pub currency: Option<Currency>,
}

impl UsageDelta {
    pub fn tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 一个组织某一天某个来源、某个币种的用量。没有单价的调用记在没有币种的那一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyUsage {
    pub day: UsageDay,
    pub origin: UsageOrigin,
    pub calls: i64,
    /// 模型服务没有填单价、没有计入成本的调用次数。
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

/// 预算用完时，有人在等的调用怎么处理。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveBudgetMode {
    /// 不再调用模型，告诉用户今天的额度用完了。
    #[default]
    Block,
    /// 照常回答，只提示超出了预算。
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

/// 组织自己的预算设置。没有这一行时全部跟随实例默认值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageBudget {
    pub org_id: Id,
    /// 每日 token 上限；`None` 跟随实例默认值，0 表示不限。
    pub daily_token_limit: Option<i64>,
    pub interactive_mode: InteractiveBudgetMode,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLevel {
    /// 没有设上限。
    Unlimited,
    Ok,
    /// 用到了提醒阈值。
    Warning,
    /// 用完了。
    Exhausted,
}

/// 一个组织今天的预算状况。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BudgetStatus {
    pub level: BudgetLevel,
    pub used_tokens: i64,
    /// 0 表示不限。
    pub limit_tokens: i64,
    pub warning_percent: u8,
    pub interactive_mode: InteractiveBudgetMode,
    /// 计数归零的时刻（下一个 UTC 零点）。
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

    /// 这次调用能不能做。后台调用额度用完就不做；有人在等的调用在"只提示"时照做。
    pub fn allows(&self, interactive: bool) -> bool {
        match self.level {
            BudgetLevel::Exhausted => {
                interactive && self.interactive_mode == InteractiveBudgetMode::Warn
            }
            BudgetLevel::Unlimited | BudgetLevel::Ok | BudgetLevel::Warning => true,
        }
    }

    /// 是否该提醒用户：接近或已经超过上限。
    pub fn should_warn(&self) -> bool {
        matches!(self.level, BudgetLevel::Warning | BudgetLevel::Exhausted)
    }

    /// 把一次调用算进已用量（缓存里的状态在下次刷新前也大致准确）。
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

/// 币种：ISO 4217 的三个大写字母，例如 `USD`、`CNY`。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

impl Currency {
    /// 没有选币种的模型服务按美元记。
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
        // 只有 `parse` 造得出来：三个 ASCII 大写字母。
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

/// 一笔成本：金额与它的币种。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: Currency,
}

impl Cost {
    /// 存成两列（金额、币种）的成本：两列都有值才算有成本。
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

/// 一次调用用掉的 token：输入里有多少是命中缓存的。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    /// 全部输入，含命中缓存的部分。
    pub prompt: i64,
    /// 输入里命中缓存的部分。
    pub cached_prompt: i64,
    pub completion: i64,
}

/// 模型服务的单价：每百万 token 的价格，用 `currency` 记。有的厂商输入分缓存命中与未命中
/// 两个价：`input_per_mtok` 是未命中（也是不分的厂商的输入价），`cached_input_per_mtok`
/// 是命中；没有填命中价时命中的输入也按输入价算。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrices {
    pub currency: Currency,
    pub input_per_mtok: f64,
    pub cached_input_per_mtok: Option<f64>,
    pub output_per_mtok: f64,
}

impl ModelPrices {
    /// 输入与输出单价都没填时没有价格；只填一个时另一个按 0 算。
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

    /// 这些 token 的成本，用 `currency` 记。
    pub fn cost(&self, tokens: TokenCounts) -> f64 {
        let cached = tokens.cached_prompt.clamp(0, tokens.prompt.max(0));
        let uncached = tokens.prompt.max(0) - cached;
        let cached_price = self.cached_input_per_mtok.unwrap_or(self.input_per_mtok);

        let input = uncached as f64 * self.input_per_mtok + cached as f64 * cached_price;
        let output = tokens.completion.max(0) as f64 * self.output_per_mtok;

        (input + output) / 1_000_000.0
    }
}

/// 单价的合法范围：非负、有限，并且不至于大到明显是填错了单位（韩元、日元的价也在内）。
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
    /// 把一次调用加进当天这个来源的计数。
    async fn add(
        &self,
        org_id: &Id,
        day: UsageDay,
        origin: UsageOrigin,
        delta: &UsageDelta,
    ) -> Result<()>;

    /// 一个组织 `from..=to` 这几天的用量，按天、来源分行，只含有调用的行。
    async fn list(&self, org_id: &Id, from: UsageDay, to: UsageDay) -> Result<Vec<DailyUsage>>;

    /// 一个组织某一天用掉的 token 总数。
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

        assert!(UsageOrigin::parse("judge").is_err());
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

        // 一百万输入里四十万命中缓存：60 万 × 2 + 40 万 × 0.5，再加 10 万输出 × 8。
        let cost = prices.cost(TokenCounts {
            prompt: 1_000_000,
            cached_prompt: 400_000,
            completion: 100_000,
        });
        assert!((cost - (1.2 + 0.2 + 0.8)).abs() < 1e-9, "{cost}");

        // 没有填命中价时，命中的输入按输入价算。
        let flat = ModelPrices::from_parts(cny, Some(2.0), None, Some(8.0)).unwrap();
        let cost = flat.cost(TokenCounts {
            prompt: 1_000_000,
            cached_prompt: 400_000,
            completion: 0,
        });
        assert!((cost - 2.0).abs() < 1e-9, "{cost}");

        // 报上来的命中数超过输入时，按全部输入命中算。
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
