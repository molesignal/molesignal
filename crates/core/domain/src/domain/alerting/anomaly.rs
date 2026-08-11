// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Anomaly detector（spec anomaly-detection capability）。
//!
//! - [`AnomalyParams`]：[`super::rule::AlertRule`] 上 `kind = Anomaly` 时携带的算法参数。
//! - [`AnomalyDetector`] trait：alert_manager evaluator 按 `AlertRule.kind` 分发。
//! - [`MadDetector`]：1.4826·MAD 鲁棒 σ 基线 + 默认 k=3 触发；按同时刻历史样本的
//!   中位数作"应该值"。其它 detector（EWMA / Prophet / IF）仅留 stub 占位。
//!
//! 该模块只做"判定"，不负责加载历史样本——evaluator 在 tick 时按 `lookback_days`
//! 反算同时刻历史窗口取来一批样本喂给 [`AnomalyDetector::detect`]。
//!
//! 纯计算（无 I/O），放在 domain 让 app 层 evaluator 可以直接调用（app 不依赖 infra）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnomalyParams {
    /// 算法。`mad` 默认；`ewma` 已实装；其它字符串走 `unimplemented` detector。
    pub algorithm: String,
    /// 回看天数，构建同时刻历史基线。
    pub lookback_days: u32,
    /// σ 倍数阈值（默认 3）。MAD 用鲁棒 σ、EWMA 用残差 σ。
    pub k: f64,
    /// EWMA 平滑系数 α∈(0,1]，越大越看重近期；仅 `algorithm = "ewma"` 使用。
    /// 老规则的持久化 JSON 没有该字段，`serde(default)` 回退到 [`default_alpha`]。
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    /// 周季节性：仅取与当前「相同星期几」的同时刻历史点构建基线，消解工作日 /
    /// 周末模式差异。默认 false（逐日取样，行为与历史一致）。开启时 evaluator 只
    /// 保留 `lookback_days` 内 `d % 7 == 0` 的历史天，建议把 lookback_days 设为
    /// ≥ 28 以获得足够样本（受 30 天上限约束最多 4 个同星期几点）。老规则的持久化
    /// JSON 没有该字段，`serde(default)` 回退 false。
    #[serde(default)]
    pub weekly_seasonality: bool,
}

/// EWMA 平滑系数默认值（0.3：兼顾平滑与对近期变化的响应）。
pub fn default_alpha() -> f64 {
    0.3
}

impl Default for AnomalyParams {
    fn default() -> Self {
        Self {
            algorithm: "mad".to_string(),
            lookback_days: 7,
            k: 3.0,
            alpha: default_alpha(),
            weekly_seasonality: false,
        }
    }
}

/// anomaly 规则允许的最大回看天数。evaluator 每个 tick 会按 `lookback_days` 各跑一次
/// 同时刻历史窗口查询，过大会放大查询压力——route 入口与 evaluator 都按此收口
/// （单一真源，evaluator 不信任持久化里的越界值）。
pub const MAX_ANOMALY_LOOKBACK_DAYS: u32 = 30;

#[derive(Debug, Clone)]
pub struct AnomalyDecision {
    pub firing: bool,
    pub current: f64,
    pub baseline: f64,
    pub deviation: f64,
    /// 0–1 异常分数：恰好触发阈值（deviation = k·σ）处约 0.5，远超趋近 1，
    /// 低于阈值 < 0.5。供 UI 排序、严重度展示与 AI 根因输入。
    pub score: f64,
    /// 人类可读的判定原因（算法、current、偏离 σ 倍数、基线）。
    pub reason: String,
}

impl AnomalyDecision {
    /// 偏离的相对量级 `deviation / max(|baseline|, ε)`——把"差多远"无量纲化，
    /// 供 incident 严重度分级使用。`baseline` 为 NaN（空历史，本就不触发）时返回 0。
    pub fn deviation_ratio(&self) -> f64 {
        if self.baseline.is_nan() {
            return 0.0;
        }
        self.deviation / self.baseline.abs().max(f64::EPSILON)
    }

    /// 历史不足以构建基线：不触发、分数 0、baseline = NaN。
    fn inconclusive(algorithm: &str, current: f64) -> Self {
        Self {
            firing: false,
            current,
            baseline: f64::NAN,
            deviation: 0.0,
            score: 0.0,
            reason: format!("{algorithm}: insufficient history for a baseline"),
        }
    }

    /// 由各 detector 算好的 `baseline` / `deviation` / `sigma`（各自含兜底）统一推导
    /// `firing` / `score` / `reason`，避免各 detector 重复阈值与恒定基线逻辑。
    fn evaluate(
        algorithm: &str,
        current: f64,
        baseline: f64,
        deviation: f64,
        sigma: f64,
        k: f64,
    ) -> Self {
        let (firing, score, reason) = if sigma > 0.0 {
            // 有界单调分数：deviation = k·σ（恰好阈值）→ 0.5，远超 → 趋近 1。
            let score = (deviation / (deviation + k * sigma)).clamp(0.0, 1.0);
            let sigma_mult = deviation / sigma;
            let reason = format!(
                "{algorithm}: current {current:.3} is {sigma_mult:.1}σ from baseline {baseline:.3} (threshold {k:.1}σ)"
            );
            (deviation > k * sigma, score, reason)
        } else {
            // 基线完全恒定（σ=0）：任何超过相对 epsilon 的偏移即异常。epsilon 随基线
            // 量级缩放，既能抓 0→500，又不被浮点噪声误触发。
            let epsilon = (baseline.abs() * 1e-6).max(f64::EPSILON);
            let firing = deviation > epsilon;
            let reason =
                format!("{algorithm}: current {current:.3} vs flat baseline {baseline:.3}");
            (firing, if firing { 1.0 } else { 0.0 }, reason)
        };
        Self {
            firing,
            current,
            baseline,
            deviation,
            score,
            reason,
        }
    }
}

pub trait AnomalyDetector: Send + Sync {
    /// `current`：本轮 evaluator 跑的当前值。
    /// `history`：同时刻历史样本，**按时间升序（最早 → 最近）**。MAD 与顺序无关，
    ///   但 EWMA 依赖时序，故 evaluator 统一按此契约喂入。
    fn detect(&self, current: f64, history: &[f64]) -> AnomalyDecision;
}

#[derive(Debug, Clone)]
pub struct MadDetector {
    pub k: f64,
}

impl MadDetector {
    pub fn new(k: f64) -> Self {
        Self { k }
    }
}

impl AnomalyDetector for MadDetector {
    fn detect(&self, current: f64, history: &[f64]) -> AnomalyDecision {
        if history.is_empty() {
            return AnomalyDecision::inconclusive("mad", current);
        }
        let median = percentile(history, 0.5);
        let abs_dev: Vec<f64> = history.iter().map(|v| (v - median).abs()).collect();
        let mad = percentile(&abs_dev, 0.5);
        let robust_sigma = 1.4826 * mad;
        let deviation = (current - median).abs();

        // 选 σ：优先 MAD 鲁棒 σ；MAD=0（低基数 / 离散计数极常见，比如 error_count
        // 长期为 0）时退回总体标准差，避免"长期为常数、突然飙升"这种最典型的异常
        // 被 robust_sigma=0 直接漏判（review medium）。σ=0（基线恒定）的 epsilon
        // 兜底由 [`AnomalyDecision::evaluate`] 统一处理。
        let sigma = if robust_sigma > 0.0 {
            robust_sigma
        } else {
            stddev(history, mean(history))
        };

        AnomalyDecision::evaluate("mad", current, median, deviation, sigma, self.k)
    }
}

/// EWMA（指数加权移动平均）控制图 detector。
///
/// 把同时刻历史序列按时间顺序平滑成 baseline——`ewma_t = α·x_t + (1-α)·ewma_{t-1}`，
/// 近期样本权重更高；σ 取一步预测残差（`x_t - ewma_{t-1}`）的标准差。
/// `|current - baseline| > k·σ` 触发。残差 σ=0（基线恒定）时退回与 [`MadDetector`]
/// 相同的相对 epsilon 兜底，保证"长期恒定突然飙升"不漏判。
///
/// 相比 MAD（同时刻取中位数作"应该值"），EWMA 对近期漂移更敏感、对单点离群更鲁棒。
#[derive(Debug, Clone)]
pub struct EwmaDetector {
    /// 平滑系数 α∈(0,1]。
    pub alpha: f64,
    /// 残差 σ 倍数阈值。
    pub k: f64,
}

impl EwmaDetector {
    pub fn new(alpha: f64, k: f64) -> Self {
        Self { alpha, k }
    }
}

impl AnomalyDetector for EwmaDetector {
    fn detect(&self, current: f64, history: &[f64]) -> AnomalyDecision {
        if history.is_empty() {
            return AnomalyDecision::inconclusive("ewma", current);
        }
        // detect 是判定层，不信任入参越界的 α（route 已收口，evaluator 旁路写入仍可能脏）。
        let alpha = self.alpha.clamp(f64::EPSILON, 1.0);
        // history 按契约时间升序：用上一拍 EWMA 作为对每个样本的一步预测，收集残差。
        let mut ewma = history[0];
        let mut residuals: Vec<f64> = Vec::with_capacity(history.len().saturating_sub(1));
        for &x in &history[1..] {
            residuals.push(x - ewma);
            ewma = alpha * x + (1.0 - alpha) * ewma;
        }
        // 纳入最后一个样本后的平滑值 = 对 current 的预测基线。
        let baseline = ewma;
        let deviation = (current - baseline).abs();

        // σ：残差总体标准差；样本不足（≤1 个残差）或残差恒为 0 时，退回历史整体
        // stddev（与 MAD 同思路），最终仍为 0 则由 evaluate 走恒定基线 epsilon 兜底。
        let resid_sigma = stddev(&residuals, mean(&residuals));
        let sigma = if resid_sigma > 0.0 {
            resid_sigma
        } else {
            stddev(history, mean(history))
        };

        AnomalyDecision::evaluate("ewma", current, baseline, deviation, sigma, self.k)
    }
}

fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

fn stddev(samples: &[f64], mean: f64) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let var = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    var.sqrt()
}

/// 线性插值分位数：`median = percentile(_, 0.5)` 取两中位元素的平均（偶数样本不再
/// 偏向较小的那个，review low）。
fn percentile(samples: &[f64], q: f64) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    let mut v: Vec<f64> = samples.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if v.len() == 1 {
        return v[0];
    }
    let rank = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return v[lo];
    }
    let frac = rank - lo as f64;
    v[lo] * (1.0 - frac) + v[hi] * frac
}

/// 未实装算法的统一占位 detector。
pub struct UnimplementedDetector(pub &'static str);

impl AnomalyDetector for UnimplementedDetector {
    fn detect(&self, current: f64, _history: &[f64]) -> AnomalyDecision {
        AnomalyDecision {
            firing: false,
            current,
            baseline: f64::NAN,
            deviation: 0.0,
            score: 0.0,
            reason: format!("{}: detector not implemented", self.0),
        }
    }
}

/// 已实装的算法名单；route 校验未支持的 detector 时复用。
pub const SUPPORTED_DETECTORS: &[&str] = &["mad", "ewma"];

/// 工厂：按 [`AnomalyParams::algorithm`] 选实装。
pub fn detector_for(params: &AnomalyParams) -> Box<dyn AnomalyDetector> {
    match params.algorithm.as_str() {
        "mad" => Box::new(MadDetector::new(params.k)),
        "ewma" => Box::new(EwmaDetector::new(params.alpha, params.k)),
        "prophet" => Box::new(UnimplementedDetector("prophet")),
        "isolation_forest" => Box::new(UnimplementedDetector("isolation_forest")),
        _ => Box::new(UnimplementedDetector("unknown")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mad_fires_on_extreme_outlier() {
        let history: Vec<f64> = (0..20).map(|i| 100.0 + (i as f64) * 0.1).collect();
        let d = MadDetector::new(3.0);
        // 远超 3σ → firing
        let r = d.detect(500.0, &history);
        assert!(r.firing);
        // 邻近基线 → 不 firing
        let r2 = d.detect(101.0, &history);
        assert!(!r2.firing);
    }

    #[test]
    fn mad_empty_history_does_not_fire() {
        let d = MadDetector::new(3.0);
        let r = d.detect(1.0, &[]);
        assert!(!r.firing);
    }

    #[test]
    fn mad_fires_on_flat_baseline_spike() {
        // 长期为 0（MAD=0、stddev=0）的离散计数突然飙到 500：必须能抓到。
        let d = MadDetector::new(3.0);
        let spike = d.detect(500.0, &[0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(
            spike.firing,
            "flat-zero baseline must still fire on a spike"
        );
        // 仍为 0 → 不触发（current==median，deviation=0）。
        let quiet = d.detect(0.0, &[0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(!quiet.firing);
        // 近似恒定但非完全恒定（stddev>0）：小抖动不触发，大跳变触发。
        let near_const = [10.0, 10.0, 10.0, 10.0, 11.0];
        assert!(!d.detect(11.0, &near_const).firing);
        assert!(d.detect(500.0, &near_const).firing);
    }

    #[test]
    fn percentile_interpolates_even_count_median() {
        // 偶数样本中位数取两中位元素平均，而非偏向较小者。
        assert!((percentile(&[48.0, 49.0, 50.0, 51.0], 0.5) - 49.5).abs() < 1e-9);
        assert!((percentile(&[1.0, 2.0, 3.0], 0.5) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn factory_dispatches_by_algorithm() {
        let mad = detector_for(&AnomalyParams::default());
        let r = mad.detect(0.0, &[1.0, 2.0, 3.0]);
        assert!(!r.firing);
        // ewma 现在分发到真实 detector：贴近平稳基线不触发。
        let ew = detector_for(&AnomalyParams {
            algorithm: "ewma".to_string(),
            ..Default::default()
        });
        let r2 = ew.detect(10.0, &[10.0, 10.0, 10.0]);
        assert!(!r2.firing);
    }

    #[test]
    fn ewma_fires_on_extreme_outlier() {
        // 噪声基线（≈100±2）上的极端尖峰必须触发。
        let d = EwmaDetector::new(0.3, 3.0);
        let history = [100.0, 102.0, 98.0, 101.0, 99.0, 100.0];
        assert!(d.detect(500.0, &history).firing);
    }

    #[test]
    fn ewma_flat_baseline_spike_and_calm() {
        // 长期恒定（残差 σ=0、stddev=0）：尖峰走 epsilon 兜底触发，等于基线不触发。
        let d = EwmaDetector::new(0.3, 3.0);
        let flat = [10.0, 10.0, 10.0, 10.0, 10.0];
        assert!(
            d.detect(500.0, &flat).firing,
            "spike over flat baseline must fire"
        );
        assert!(
            !d.detect(10.0, &flat).firing,
            "value equal to baseline must not fire"
        );
    }

    #[test]
    fn ewma_empty_history_does_not_fire() {
        let d = EwmaDetector::new(0.3, 3.0);
        assert!(!d.detect(1.0, &[]).firing);
    }

    #[test]
    fn ewma_alpha_weights_recent_samples() {
        // 高 α 更看重近期：经历 0→100 阶跃后，基线更快贴近新水平。
        let history = [0.0, 0.0, 0.0, 100.0, 100.0, 100.0];
        let high = EwmaDetector::new(0.8, 3.0).detect(100.0, &history).baseline;
        let low = EwmaDetector::new(0.2, 3.0).detect(100.0, &history).baseline;
        assert!(high > low, "higher alpha tracks the recent step faster");
        assert!(
            high > 90.0,
            "alpha=0.8 baseline should sit near the recent 100 level"
        );
    }

    #[test]
    fn score_is_bounded_monotonic_and_explained() {
        // 噪声基线（σ>0）：分数随偏离单调增、恒在 [0,1]、触发即 > 0.5，且带原因文案。
        let d = MadDetector::new(3.0);
        let history: Vec<f64> = (0..20).map(|i| 100.0 + (i % 5) as f64).collect();
        let near = d.detect(101.0, &history);
        let far = d.detect(500.0, &history);
        assert!((0.0..=1.0).contains(&near.score));
        assert!((0.0..=1.0).contains(&far.score));
        assert!(far.score > near.score, "larger deviation → larger score");
        assert!(far.firing && far.score > 0.5, "firing implies score > 0.5");
        assert!(!near.firing && near.score < 0.5, "calm implies score < 0.5");
        assert!(far.reason.contains("mad"), "reason names the algorithm");
        // 空历史：inconclusive，分数 0、不触发、原因说明历史不足。
        let none = d.detect(1.0, &[]);
        assert!(!none.firing && none.score == 0.0);
        assert!(none.reason.contains("insufficient history"));
    }

    // #13 跨集群 anomaly 同步：anomaly 类 AlertRule 随 AlertRule CUD 事件传播；
    // anomaly_params 内嵌在 AlertRule，必须完整穿过 CloudEvent 信封的 JSON 往返，
    // 接收端用 detector_for(&params) 即可实例化评估器（MAD/EWMA 无状态、查本地历史）。
    #[test]
    fn anomaly_alert_rule_survives_cross_cluster_round_trip() {
        use std::collections::BTreeMap;

        use crate::{
            domain::{
                alerting::rule::{
                    AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, ComparisonOp, RuleState,
                },
                federation::{CloudEvent, CudAction, ResourceKind, parse_event_type},
                query::QueryLanguage,
            },
            shared::{ids::Id, time::TimestampMicros},
        };

        let rule = AlertRule {
            id: Id::from_string("rule-1"),
            org_id: Id::from_string("orgA"),
            name: "latency anomaly".into(),
            description: "detect latency spikes".into(),
            enabled: true,
            kind: AlertRuleKind::Anomaly,
            query: AlertQuery {
                language: QueryLanguage::Sql,
                statement: "SELECT avg(latency) FROM logs".into(),
                period_secs: 60,
                stream: None,
            },
            trigger: AlertTrigger {
                operator: ComparisonOp::Gt,
                threshold: 0.0,
                for_periods: 1,
                silence_secs: 300,
            },
            anomaly_params: Some(AnomalyParams {
                algorithm: "ewma".into(),
                lookback_days: 14,
                k: 2.5,
                alpha: 0.42,
                weekly_seasonality: true,
            }),
            thresholds: vec![],
            severity: None,
            escalation_policy_id: Id::from_string("esc-1"),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(2),
        };

        // emit：AlertRule → CloudEvent.data（与 `emit_cud` 的 to_value 一致）。
        let data = serde_json::to_value(&rule).unwrap();
        let ev = CloudEvent::new(
            "evt-1".into(),
            "clusterA",
            ResourceKind::AlertRule,
            CudAction::Created,
            &rule.org_id.0,
            &rule.id.0,
            7,
            data,
            "2026-06-15T00:00:00Z".into(),
        );
        // 线缆往返：整条 CloudEvent 序列化 → bytes → 反序列化（接收端解码路径）。
        let bytes = serde_json::to_vec(&ev).unwrap();
        let back: CloudEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            parse_event_type(&back.event_type),
            Some((ResourceKind::AlertRule, CudAction::Created))
        );

        // apply：CloudEvent.data → AlertRule（接收端反序列化路径）。
        let recovered: AlertRule = serde_json::from_value(back.data).unwrap();
        assert_eq!(recovered.kind, AlertRuleKind::Anomaly);
        let params = recovered.anomaly_params.expect("anomaly_params survives");
        assert_eq!(params.algorithm, "ewma");
        assert_eq!(params.lookback_days, 14);
        assert_eq!(params.k, 2.5);
        assert_eq!(params.alpha, 0.42);
        assert!(
            params.weekly_seasonality,
            "weekly_seasonality survives cross-cluster round-trip"
        );

        // detector_for(&params) 可实例化并评估（current 透传证明 detector 真在跑）。
        let detector = detector_for(&params);
        let decision = detector.detect(10.0, &[1.0, 1.0, 1.0, 1.0]);
        assert_eq!(decision.current, 10.0);
    }
}
