// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询优化建议引擎（启发式，纯逻辑）。
//!
//! 给定一次查询的画像（语句 + 执行统计），产出可操作的优化建议。纯函数、可单测、
//! 不触外部依赖（domain-only），符合 domain ← app ← infra 方向。HTTP
//! `POST /query/recommendations` 以无状态方式调用；统计通常取自上一次查询响应的
//! `scanned_rows` / `took_ms`。未来也可由周期任务批量分析慢查询（需先落慢查询采集）。

use serde::{Deserialize, Serialize};

use crate::domain::query::{QueryLanguage, SlowQuery};

/// 慢查询毫秒阈值。
const SLOW_MS: u64 = 3_000;
/// "大扫描"行数阈值。
const LARGE_SCAN: u64 = 100_000;
/// "宽时间窗口"秒阈值（7 天）。
const WIDE_RANGE_SECS: i64 = 7 * 86_400;
/// 低选择度（扫描/返回）比阈值。
const LOW_SELECTIVITY_RATIO: u64 = 1_000;
/// 无 LIMIT 时判定"返回过多"的行数阈值。
const LARGE_RETURN: u64 = 10_000;
/// PromQL range selector "宽窗口"秒阈值（1 天）。
const WIDE_PROMQL_RANGE_SECS: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationSeverity {
    Info,
    Warning,
    Critical,
}

/// 一条优化建议。`code` 稳定，前端可用于 i18n / 去重。
#[derive(Debug, Clone, Serialize)]
pub struct QueryRecommendation {
    pub code: String,
    pub severity: RecommendationSeverity,
    pub title: String,
    pub detail: String,
}

/// 查询画像：语句 + 执行统计。统计通常取自上一次查询响应。
#[derive(Debug, Clone, Deserialize)]
pub struct QueryProfile {
    pub language: QueryLanguage,
    pub statement: String,
    #[serde(default)]
    pub scanned_rows: u64,
    #[serde(default)]
    pub returned_rows: u64,
    #[serde(default)]
    pub took_ms: u64,
    /// 查询时间窗口（秒）；None = 未指定/未知（保守视作潜在宽窗口）。
    #[serde(default)]
    pub time_range_secs: Option<i64>,
}

fn rec(
    code: &str,
    severity: RecommendationSeverity,
    title: &str,
    detail: String,
) -> QueryRecommendation {
    QueryRecommendation {
        code: code.into(),
        severity,
        title: title.into(),
        detail,
    }
}

/// 启发式分析，产出优化建议（可能为空）。建议是顾问性的，不改变查询本身。
pub fn analyze(p: &QueryProfile) -> Vec<QueryRecommendation> {
    use RecommendationSeverity::{Info, Warning};
    let mut out = Vec::new();
    let stmt = p.statement.to_lowercase();
    let is_sql = matches!(p.language, QueryLanguage::Sql);

    if p.took_ms >= SLOW_MS {
        out.push(rec(
            "slow_query",
            Warning,
            "Slow query",
            format!(
                "Query took {} ms scanning {} rows. Narrow the time range or add more selective filters.",
                p.took_ms, p.scanned_rows
            ),
        ));
    }

    // None（未知窗口）保守视作宽窗口：缺时间界定本身就值得提示。
    let wide_window = p
        .time_range_secs
        .map(|s| s > WIDE_RANGE_SECS)
        .unwrap_or(true);
    if p.scanned_rows >= LARGE_SCAN && wide_window {
        out.push(rec(
            "wide_time_range",
            Warning,
            "Wide scan window",
            "Large scan over an unbounded or multi-week window. Restrict the query to the \
             smallest time range that answers your question."
                .into(),
        ));
    }

    if p.scanned_rows >= LARGE_SCAN
        && p.returned_rows > 0
        && p.scanned_rows / p.returned_rows >= LOW_SELECTIVITY_RATIO
    {
        out.push(rec(
            "low_selectivity",
            Info,
            "Low selectivity",
            format!(
                "Scanned {} rows to return {}. Add predicates on indexed/partition fields so fewer rows are read.",
                p.scanned_rows, p.returned_rows
            ),
        ));
    }

    if is_sql && stmt.contains("select *") {
        out.push(rec(
            "select_star",
            Info,
            "Avoid SELECT *",
            "Projecting all columns reads more data than needed. Select only the columns you use."
                .into(),
        ));
    }

    if is_sql && !stmt.contains(" where") && p.scanned_rows >= LARGE_SCAN {
        out.push(rec(
            "missing_filter",
            Warning,
            "No filter predicate",
            "A large scan without a WHERE clause reads the whole window. Add a filter to cut IO."
                .into(),
        ));
    }

    if is_sql && !stmt.contains("limit") && p.returned_rows >= LARGE_RETURN {
        out.push(rec(
            "missing_limit",
            Info,
            "Add a LIMIT",
            format!(
                "Returned {} rows without a LIMIT. Add one if you only need a sample or the top results.",
                p.returned_rows
            ),
        ));
    }

    // PromQL 专属启发式（字符串级，顾问性）。
    if matches!(p.language, QueryLanguage::Promql) {
        if let Some(secs) = max_range_selector_secs(&p.statement)
            && secs >= WIDE_PROMQL_RANGE_SECS
        {
            out.push(rec(
                "promql_wide_range",
                Info,
                "Wide range selector",
                format!(
                    "A range selector of ~{secs}s reads many samples per series. Consider a recording rule or a smaller window."
                ),
            ));
        }
        if has_subquery(&p.statement) {
            out.push(rec(
                "promql_subquery",
                Info,
                "Subquery cost",
                "Subqueries ([..:..]) re-evaluate the inner query at every step and are expensive. \
                 Precompute hot ones with a recording rule."
                    .into(),
            ));
        }
        if p.statement.contains("=~") || p.statement.contains("!~") {
            out.push(rec(
                "promql_regex_matcher",
                Info,
                "Regex label matcher",
                "Regex matchers (=~ / !~) scan more series than exact matchers. Use = / != where \
                 possible, or anchor the pattern."
                    .into(),
            ));
        }
    }

    out
}

/// 规范化语句（折叠空白 + 小写）后的稳定指纹（blake3），用于慢查询去重。
pub fn query_fingerprint(language: QueryLanguage, statement: &str) -> String {
    let normalized = statement
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let lang = match language {
        QueryLanguage::Sql => "sql",
        QueryLanguage::Promql => "promql",
    };
    blake3::hash(format!("{lang}:{normalized}").as_bytes())
        .to_hex()
        .to_string()
}

/// 提取 PromQL 中所有 `[..]` 段的内容（不含方括号）。
fn bracket_segments(stmt: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = stmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(rel) = stmt[i + 1..].find(']')
        {
            out.push(&stmt[i + 1..i + 1 + rel]);
            i = i + 1 + rel + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// `[..:..]` 形式（含 `:`）即 subquery。
fn has_subquery(stmt: &str) -> bool {
    bracket_segments(stmt).iter().any(|seg| seg.contains(':'))
}

/// 各 `[..]` 段里（subquery 取 `:` 之前的区间部分）的最大时长（秒）。
fn max_range_selector_secs(stmt: &str) -> Option<i64> {
    let mut max: Option<i64> = None;
    for seg in bracket_segments(stmt) {
        let dur = seg.split(':').next().unwrap_or(seg).trim();
        if let Some(secs) = parse_promql_duration(dur) {
            max = Some(max.map_or(secs, |m| m.max(secs)));
        }
    }
    max
}

/// 解析 PromQL 时长（如 `5m` / `1h30m` / `2d`）为秒；非法返回 None。
fn parse_promql_duration(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let mut total = 0i64;
    let mut num = String::new();
    let mut any = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            let unit_secs = match ch {
                's' => 1,
                'm' => 60,
                'h' => 3_600,
                'd' => 86_400,
                'w' => 604_800,
                'y' => 31_536_000,
                _ => return None,
            };
            total += n * unit_secs;
            any = true;
        }
    }
    // 末尾残留数字（无单位）或整体无单位 → 非法。
    if !num.is_empty() || !any {
        return None;
    }
    Some(total)
}

impl From<&SlowQuery> for QueryProfile {
    fn from(sq: &SlowQuery) -> Self {
        QueryProfile {
            language: sq.language,
            statement: sq.statement.clone(),
            scanned_rows: sq.scanned_rows.max(0) as u64,
            returned_rows: sq.returned_rows.max(0) as u64,
            took_ms: sq.took_ms.max(0) as u64,
            time_range_secs: sq.time_range_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(stmt: &str) -> QueryProfile {
        QueryProfile {
            language: QueryLanguage::Sql,
            statement: stmt.into(),
            scanned_rows: 0,
            returned_rows: 0,
            took_ms: 0,
            time_range_secs: Some(3600),
        }
    }

    fn codes(recs: &[QueryRecommendation]) -> Vec<&str> {
        recs.iter().map(|r| r.code.as_str()).collect()
    }

    #[test]
    fn clean_fast_query_has_no_recommendations() {
        let mut p = profile("SELECT level FROM logs WHERE level = 'error' LIMIT 100");
        p.scanned_rows = 500;
        p.returned_rows = 100;
        p.took_ms = 40;
        assert!(analyze(&p).is_empty());
    }

    #[test]
    fn slow_query_flagged() {
        let mut p = profile("SELECT level FROM logs WHERE x LIMIT 10");
        p.took_ms = 5_000;
        assert!(codes(&analyze(&p)).contains(&"slow_query"));
    }

    #[test]
    fn wide_scan_and_low_selectivity() {
        let mut p = profile("SELECT a FROM logs WHERE a > 0 LIMIT 5");
        p.scanned_rows = 5_000_000;
        p.returned_rows = 10;
        p.time_range_secs = Some(30 * 86_400);
        let recs = analyze(&p);
        let c = codes(&recs);
        assert!(c.contains(&"wide_time_range"));
        assert!(c.contains(&"low_selectivity"));
    }

    #[test]
    fn select_star_missing_filter_and_limit() {
        let mut p = profile("SELECT * FROM logs");
        p.scanned_rows = 2_000_000;
        p.returned_rows = 50_000;
        let recs = analyze(&p);
        let c = codes(&recs);
        assert!(c.contains(&"select_star"));
        assert!(c.contains(&"missing_filter"));
        assert!(c.contains(&"missing_limit"));
    }

    #[test]
    fn unknown_window_treated_as_wide() {
        let mut p = profile("SELECT a FROM logs WHERE a LIMIT 1");
        p.scanned_rows = 200_000;
        p.time_range_secs = None;
        assert!(codes(&analyze(&p)).contains(&"wide_time_range"));
    }

    #[test]
    fn promql_skips_sql_only_rules() {
        let mut p = profile("rate(http_requests_total[5m])");
        p.language = QueryLanguage::Promql;
        p.scanned_rows = 2_000_000;
        p.returned_rows = 50_000;
        let recs = analyze(&p);
        let c = codes(&recs);
        // SQL 专属规则不触发；但通用的大扫描/选择度仍可触发。
        assert!(!c.contains(&"select_star"));
        assert!(!c.contains(&"missing_filter"));
        assert!(!c.contains(&"missing_limit"));
    }

    fn promql(stmt: &str) -> QueryProfile {
        QueryProfile {
            language: QueryLanguage::Promql,
            statement: stmt.into(),
            scanned_rows: 0,
            returned_rows: 0,
            took_ms: 0,
            time_range_secs: Some(3600),
        }
    }

    #[test]
    fn promql_specific_rules_fire() {
        assert!(codes(&analyze(&promql("avg_over_time(cpu[2d])"))).contains(&"promql_wide_range"));
        let regex = analyze(&promql("rate(http{job=~\".*\"}[5m])"));
        assert!(codes(&regex).contains(&"promql_regex_matcher"));
        let sub = analyze(&promql("max_over_time(rate(http[1m])[1h:1m])"));
        assert!(codes(&sub).contains(&"promql_subquery"));
    }

    #[test]
    fn promql_short_range_is_not_flagged() {
        let c = analyze(&promql("rate(http[5m])"));
        assert!(codes(&c).is_empty());
    }

    #[test]
    fn parse_duration_units_and_combos() {
        assert_eq!(parse_promql_duration("5m"), Some(300));
        assert_eq!(parse_promql_duration("1h30m"), Some(5_400));
        assert_eq!(parse_promql_duration("2d"), Some(172_800));
        assert_eq!(parse_promql_duration("5"), None); // 无单位
        assert_eq!(parse_promql_duration(""), None);
        assert_eq!(parse_promql_duration("abc"), None);
    }

    #[test]
    fn fingerprint_normalizes_and_separates_languages() {
        let a = query_fingerprint(QueryLanguage::Sql, "SELECT *   FROM logs");
        let b = query_fingerprint(QueryLanguage::Sql, "select * from logs");
        assert_eq!(a, b); // 折叠空白 + 大小写无关
        assert_ne!(
            a,
            query_fingerprint(QueryLanguage::Promql, "select * from logs")
        );
    }
}
