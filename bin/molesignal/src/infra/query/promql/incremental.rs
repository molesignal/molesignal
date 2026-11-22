// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! range PromQL 的按窗口分桶增量缓存接入层（缓存类型见
//! [`crate::infra::caching::streaming_agg`]）。
//!
//! 思路：range 求值点就是请求的步点 `start + n*step`。同一 step、同一网格相位
//! （`start mod step`）的请求步点落在同一套网格上，仪表盘滑动刷新（start 平移整数倍 step）
//! 时相邻两次查询的步点重合 → 可跨刷新复用；相位不同的请求进不同的缓存条目，绝不共用
//! 求值结果。每个网格桶按「窗口右端 vs 水位」分稳定 / 活跃：
//! - **稳定**（`t <= watermark`）：值已封存，入缓存、跨刷新命中。
//! - **活跃**（`t > watermark`）：每次重算、不缓存（规避晚到数据）。
//!
//! 深度增量：刷新时**只扫活跃 + 未缓存稳定区** `[compute_lo - range, end]` 的 parquet，
//! 已封存稳定桶整段从缓存出，不触碰对象存储。停报 / 新出现的 series 由
//! [`SealedSeries`] 的「封存区间全 series 覆盖」不变量保证不漏不重。
//!
//! `@` / `offset` 修饰的选择器不走缓存（回退原 range 路径），避免与生效时刻平移的交互。

// 其余类型（Arc/Duration/SystemTime/UNIX_EPOCH/StreamType/MatchOp/VectorSelector/Expr/
// LabelSet/Series/RangePoint/RangeVector/each_step_window/range_step_us/QueryRequest/Result…）
// 经 `use super::*` 从 promql 模块作用域带入，与其余子模块一致。
use std::sync::RwLock;

use super::*;
use crate::infra::caching::{SealedSeries, StreamingAggCache};

/// 引擎持有的增量缓存装配：缓存句柄 + 安全回看窗口（micros）。
pub(super) struct StreamingAgg {
    pub(super) cache: Arc<StreamingAggCache>,
    pub(super) safe_lookback_us: i64,
}

/// 把时间戳向下取到相位为 `phase` 的 step 网格：最后一个 `<= t` 的 `phase + n*step`。
fn grid_floor(t: i64, step_us: i64, phase: i64) -> i64 {
    let step = step_us.max(1);
    let shifted = t - phase;

    shifted - shifted.rem_euclid(step) + phase
}

fn match_op_tag(op: &MatchOp) -> &'static str {
    match op {
        MatchOp::Equal => "=",
        MatchOp::NotEqual => "!=",
        MatchOp::Re(_) => "=~",
        MatchOp::NotRe(_) => "!~",
    }
}

/// 查询指纹：`blake3(org + 函数指纹 + 选择器 + window + step + 网格相位)`。选择器 matcher
/// 经规范化排序，使语义相同但书写顺序不同的查询命中同一缓存。`step` 与相位
/// （`start mod step`）入指纹是因为二者决定求值时刻——任一不同即另一套桶，不可混用。
fn selector_fingerprint(
    org_id: &str,
    func_key: &str,
    vs: &VectorSelector,
    range_us: i64,
    step_us: i64,
    phase: i64,
) -> String {
    let mut matcher_parts: Vec<String> = vs
        .matchers
        .matchers
        .iter()
        .map(|m| format!("{}{}{}", m.name, match_op_tag(&m.op), m.value))
        .collect();

    matcher_parts.sort();

    let mut or_groups: Vec<String> = vs
        .matchers
        .or_matchers
        .iter()
        .map(|g| {
            let mut items: Vec<String> = g
                .iter()
                .map(|m| format!("{}{}{}", m.name, match_op_tag(&m.op), m.value))
                .collect();

            items.sort();
            format!("[{}]", items.join(","))
        })
        .collect();

    or_groups.sort();

    let mut s = String::new();
    s.push_str(org_id);
    s.push('\u{1}');
    s.push_str(func_key);
    s.push('\u{1}');
    s.push_str("name=");
    s.push_str(vs.name.as_deref().unwrap_or(""));
    s.push('\u{1}');
    s.push_str(&range_us.to_string());
    s.push('\u{1}');
    s.push_str(&step_us.to_string());
    s.push('\u{1}');
    s.push_str(&phase.to_string());

    for p in matcher_parts {
        s.push('\u{2}');
        s.push_str(&p);
    }

    for g in or_groups {
        s.push('\u{3}');
        s.push_str(&g);
    }

    blake3::hash(s.as_bytes()).to_hex().to_string()
}

/// 一次缓存查询的重算计划。
#[derive(Debug, PartialEq)]
struct ComputePlan {
    /// 需重算（含活跃区 + 未缓存稳定区）的 grid 桶下界；`> q_hi` 表示无需扫描。
    compute_lo: i64,
    /// 可直接从缓存服务的 grid 桶闭区间 `[lo, hi]`；`None` 表示全部重算。
    serve: Option<(i64, i64)>,
}

/// 给定缓存已封存区间 `[stable_from, stable_through]` 与本次查询的网格边界
/// `[q_lo, q_hi]` + 已落到网格上的水位 `watermark`，规划「服务哪段缓存、从哪个桶起重算」。
/// 所有入参都在同一套网格上。
///
/// - 查询下界落在缓存覆盖之下（`q_lo < stable_from`）→ 全量重算、重新封存这段。
/// - 否则仅信任 `[stable_from, min(stable_through, watermark)]`（水位回退时强制重算
///   `(watermark, stable_through]`），只服务其中落在 `[q_lo, q_hi]` 的部分，其余从
///   `min(through, wm) + step` 起重算。
fn plan_compute_range(
    stable_from: i64,
    stable_through: i64,
    q_lo: i64,
    q_hi: i64,
    watermark: i64,
    step_us: i64,
) -> ComputePlan {
    if q_lo < stable_from {
        return ComputePlan {
            compute_lo: q_lo,
            serve: None,
        };
    }

    let trusted = stable_through.min(watermark);
    let serve_hi = trusted.min(q_hi);

    let serve = if q_lo <= serve_hi {
        Some((q_lo, serve_hi))
    } else {
        None
    };

    let compute_lo = q_lo
        .max(trusted.saturating_add(step_us))
        .min(q_hi.saturating_add(step_us));

    ComputePlan { compute_lo, serve }
}

fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

impl PromQLEngine {
    /// 选择器是否可走增量缓存：缓存已装配、是裸 matrix selector、无 `@`/`offset`、有 metric 名。
    pub(super) fn cacheable_matrix<'a>(
        &self,
        arg: &'a Expr,
    ) -> Option<&'a promql_parser::parser::MatrixSelector> {
        self.streaming.as_ref()?;

        match arg {
            Expr::MatrixSelector(ms)
                if ms.vs.at.is_none() && ms.vs.offset.is_none() && ms.vs.name.is_some() =>
            {
                Some(ms)
            }
            _ => None,
        }
    }

    /// 该 metric 在查询窗口内的 intake 水位约等于持有它的各流里
    /// `max(Catalog Segment time_range.end, visible Buffer _timestamp)`；无数据返回
    /// `i64::MIN`（→ 全活跃、无可封存桶）。Buffer 里是整个流的行，水位可能偏晚，只会让活跃区
    /// 更大。Catalog 与 Buffer 来自同一次 checkpoint 去重后的查询快照，且不会读取 WAL。
    async fn intake_watermark(&self, vs: &VectorSelector, req: &QueryRequest) -> Result<i64> {
        let filter = MetricFilter::of(vs);
        let sources = self
            .metric_sources(&req.org_id, &filter, req.time_range)
            .await?;
        let mut watermark = i64::MIN;

        for source in sources {
            let storage = self
                .metric_files(&req.org_id, &source.stream, req.time_range)
                .await?;

            let file_watermark = storage.files.iter().map(|m| m.time_range.end.0).max();

            let buffer_watermark = storage
                .buffered_batches
                .iter()
                .filter_map(|batch| {
                    batch
                        .column_by_name(crate::infra::storage::arrow_schema::TS_COL)?
                        .as_any()
                        .downcast_ref::<TimestampMicrosecondArray>()?
                        .iter()
                        .flatten()
                        .max()
                })
                .max();

            watermark = file_watermark
                .into_iter()
                .chain(buffer_watermark)
                .fold(watermark, i64::max);
        }

        Ok(watermark)
    }

    /// range 函数（rate / `*_over_time` / delta 等）的增量缓存求值。`compute` 是纯窗口
    /// 求值（`(step_ts, 窗口样本, 样本元数据) -> Option<value>`），与未缓存路径共用
    /// 同一份逻辑，保证同一 (step_ts, 窗口) 取值一致——只输出请求的步点 `start + n*step`。
    pub(super) async fn eval_windowed_cached<F>(
        &self,
        vs: &VectorSelector,
        range: Duration,
        func_key: &str,
        req: &QueryRequest,
        compute: F,
    ) -> Result<RangeVector>
    where
        F: Fn(i64, &[(i64, f64)], &[MetricSampleMetadata]) -> Option<f64>,
    {
        let agg = self
            .streaming
            .as_ref()
            .expect("eval_windowed_cached requires streaming cache");

        let cache = &agg.cache;
        let step_us = range_step_us(req);
        let range_us = (range.as_micros() as i64).max(1);
        let q_lo = req.time_range.start.0;
        let phase = q_lo.rem_euclid(step_us);
        let q_hi = grid_floor(req.time_range.end.0.max(q_lo), step_us, phase);

        // 水位 = min(now - safe_lookback, intake 水位)，落到请求网格上：网格点 t 稳定当且仅当
        // t <= watermark。早于 q_lo 的水位一律按 q_lo - step 算（本次无可封存桶）。
        let intake_wm = self.intake_watermark(vs, req).await?;

        let watermark = now_micros()
            .saturating_sub(agg.safe_lookback_us)
            .min(intake_wm)
            .max(q_lo.saturating_sub(step_us));

        let watermark = grid_floor(watermark, step_us, phase);

        let fp = selector_fingerprint(
            req.org_id.0.as_str(),
            func_key,
            vs,
            range_us,
            step_us,
            phase,
        );
        let entry = cache.get(&fp).await;

        // 规划：从快照读已封存区间，决定 compute_lo / serve。
        let plan = match &entry {
            Some(e) => {
                let g = e.read().unwrap();

                if g.is_empty() {
                    ComputePlan {
                        compute_lo: q_lo,
                        serve: None,
                    }
                } else {
                    plan_compute_range(
                        g.stable_from(),
                        g.stable_through(),
                        q_lo,
                        q_hi,
                        watermark,
                        step_us,
                    )
                }
            }
            None => ComputePlan {
                compute_lo: q_lo,
                serve: None,
            },
        };

        let mut points: Vec<RangePoint> = Vec::new();
        let mut sealed: std::collections::HashMap<LabelSet, Vec<(i64, f64)>> =
            std::collections::HashMap::new();
        let mut miss_points = 0u64;

        // 本次重算覆盖的稳定网格桶区间：区间里每个桶的所有 series 都算过，可整段封存
        // （没有点的桶也算覆盖过，之后照样服务成「无点」）。
        let seal_lo = plan.compute_lo;
        let seal_hi = q_hi.min(watermark);

        // 重算活跃 + 未缓存稳定区：只扫 [compute_lo - range, end]。
        if plan.compute_lo <= q_hi {
            let scan_lo = plan.compute_lo.saturating_sub(range_us);
            let total_us = req.time_range.end.0.saturating_sub(scan_lo).max(1);

            let series = self
                .load_matrix(
                    vs,
                    Duration::from_micros(total_us as u64),
                    req.time_range.end.0,
                    req,
                )
                .await?;

            for s in &series {
                each_step_window_indices(
                    &s.samples,
                    Steps::new(plan.compute_lo, q_hi, step_us),
                    range_us,
                    |t, _, lo, hi| {
                        let samples = &s.samples[lo..hi];
                        let metadata = s.metadata_slice(lo, hi);

                        let Some(v) = compute(t, samples, metadata) else {
                            return;
                        };

                        points.push(RangePoint {
                            ts_us: t,
                            labels: s.labels.clone(),
                            value: v,
                        });

                        if t <= seal_hi {
                            sealed.entry(s.labels.clone()).or_default().push((t, v));
                            miss_points += 1;
                        }
                    },
                );
            }
        }

        // 服务已封存稳定桶（不触碰对象存储）。
        let mut hit_points = 0u64;

        if let (Some((serve_lo, serve_hi)), Some(e)) = (plan.serve, &entry) {
            let g = e.read().unwrap();

            hit_points = g.serve(serve_lo, serve_hi, |labels, ts, v| {
                points.push(RangePoint {
                    ts_us: ts,
                    labels: labels.clone(),
                    value: v,
                });
            });
        }

        cache.record_hits(hit_points);
        cache.record_misses(miss_points);

        // 回填本次新封存的桶。
        if plan.compute_lo <= q_hi && seal_lo <= seal_hi {
            let new_buckets: Vec<(LabelSet, Vec<(i64, f64)>)> = sealed.into_iter().collect();
            let max_buckets = cache.max_buckets_per_series();
            let max_series = cache.max_series_per_query();

            match &entry {
                Some(e) => {
                    e.write().unwrap().seal(
                        new_buckets,
                        seal_lo,
                        seal_hi,
                        step_us,
                        max_buckets,
                        max_series,
                    );

                    // 缓存按条目的大小计重：封存了新桶的条目放回一次重新计重。
                    cache.reweigh(fp, e.clone()).await;
                }
                None => {
                    let mut fresh = SealedSeries::default();

                    fresh.seal(
                        new_buckets,
                        seal_lo,
                        seal_hi,
                        step_us,
                        max_buckets,
                        max_series,
                    );

                    cache.insert(fp, Arc::new(RwLock::new(fresh))).await;
                }
            }
        }

        Ok(RangeVector { points })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_floor_keeps_the_request_phase() {
        assert_eq!(grid_floor(100, 50, 0), 100);
        assert_eq!(grid_floor(149, 50, 0), 100);
        assert_eq!(grid_floor(150, 50, 0), 150);
        assert_eq!(grid_floor(149, 50, 25), 125);
        assert_eq!(grid_floor(124, 50, 25), 75);
        assert_eq!(grid_floor(-1, 50, 25), -25);
    }

    /// 诊断里的原始请求：start 不在整分钟上、5 分钟、step 60s，应有 6 个步点，
    /// 第一个就是 start，而不是下一个整分钟。
    #[test]
    fn request_grid_starts_at_start_and_keeps_every_step() {
        let start = 1_793_845_125_000_000_i64;
        let end = 1_793_845_425_000_000_i64;
        let step = 60_000_000_i64;
        let phase = start.rem_euclid(step);
        let q_hi = grid_floor(end, step, phase);

        assert_eq!(q_hi, end);
        assert_eq!((q_hi - start) / step + 1, 6);
    }

    #[test]
    fn plan_cold_below_range_recomputes_all() {
        // 查询下界落在缓存覆盖之下 → 全量重算、不服务缓存。
        let p = plan_compute_range(500, 1000, 300, 1300, 1100, 100);
        assert_eq!(
            p,
            ComputePlan {
                compute_lo: 300,
                serve: None
            }
        );
    }

    #[test]
    fn plan_steady_slide_only_recomputes_top() {
        // 缓存 [100,1000]，窗口前移到 [300,1300]，水位推进到 1100。
        let p = plan_compute_range(100, 1000, 300, 1300, 1100, 100);
        // 信任段 = min(1000, 1100) = 1000 → 服务 [300,1000]；从 1100 起重算。
        assert_eq!(
            p,
            ComputePlan {
                compute_lo: 1100,
                serve: Some((300, 1000))
            }
        );
    }

    #[test]
    fn plan_never_serves_past_the_query_end() {
        // 先查过大窗口，缓存已封存到 5000；再查小窗口 [300,700]，只能服务到 700。
        let p = plan_compute_range(100, 5000, 300, 700, 6000, 100);

        assert_eq!(p.serve, Some((300, 700)));
        assert!(p.compute_lo > 700, "nothing left to compute");
    }

    #[test]
    fn plan_no_slide_refresh_serves_all() {
        // 同一窗口再查一次、且上次已全封存（through == q_hi <= watermark）→ 全服务、零重算。
        let p = plan_compute_range(100, 1300, 300, 1300, 5000, 100);
        assert_eq!(p.serve, Some((300, 1300)));
        assert!(p.compute_lo > 1300, "compute_lo past q_hi → no scan");
    }

    #[test]
    fn plan_watermark_backward_forces_recompute_of_now_active() {
        // 缓存 through=1000，但水位回退到 800 → (800,1000] 重新算（视为活跃），
        // 只服务 [q_lo, 800]。
        let p = plan_compute_range(100, 1000, 100, 1300, 800, 100);
        assert_eq!(
            p,
            ComputePlan {
                compute_lo: 900,
                serve: Some((100, 800))
            }
        );
    }

    #[test]
    fn fingerprint_is_stable_and_separates_queries() {
        let (vs_a, r_a) = vs_of("rate(http_requests_total{method=\"GET\",code=\"200\"}[1m])");
        // matcher 书写顺序不同，语义相同 → 同指纹。
        let (vs_b, r_b) = vs_of("rate(http_requests_total{code=\"200\",method=\"GET\"}[1m])");
        let fa = selector_fingerprint("org", "rate", &vs_a, r_a.as_micros() as i64, 1000, 0);
        let fb = selector_fingerprint("org", "rate", &vs_b, r_b.as_micros() as i64, 1000, 0);
        assert_eq!(fa, fb, "matcher order must not change fingerprint");

        // 不同 step / 网格相位 / window / 函数 / org → 不同指纹。
        assert_ne!(
            fa,
            selector_fingerprint("org", "rate", &vs_a, r_a.as_micros() as i64, 2000, 0)
        );
        assert_ne!(
            fa,
            selector_fingerprint("org", "rate", &vs_a, r_a.as_micros() as i64, 1000, 500),
            "same step, different phase → different evaluation times"
        );
        assert_ne!(
            fa,
            selector_fingerprint("org", "increase", &vs_a, r_a.as_micros() as i64, 1000, 0)
        );
        assert_ne!(
            fa,
            selector_fingerprint("org2", "rate", &vs_a, r_a.as_micros() as i64, 1000, 0)
        );
        let (vs_c, r_c) = vs_of("rate(http_requests_total{method=\"GET\",code=\"200\"}[5m])");
        assert_ne!(
            fa,
            selector_fingerprint("org", "rate", &vs_c, r_c.as_micros() as i64, 1000, 0),
            "different window → different fingerprint"
        );
        // 不同 metric → 不同指纹。
        let (vs_d, r_d) = vs_of("rate(other_metric{method=\"GET\",code=\"200\"}[1m])");
        assert_ne!(
            fa,
            selector_fingerprint("org", "rate", &vs_d, r_d.as_micros() as i64, 1000, 0)
        );
    }

    fn vs_of(promql: &str) -> (VectorSelector, Duration) {
        let expr = promql_parser::parser::parse(promql).expect("parse");
        match expr {
            Expr::Call(call) => {
                let arg = call.args.args.into_iter().next().expect("one arg");
                match *arg {
                    Expr::MatrixSelector(ms) => (ms.vs, ms.range),
                    other => panic!("expected matrix selector, got {other:?}"),
                }
            }
            other => panic!("expected call, got {other:?}"),
        }
    }
}
