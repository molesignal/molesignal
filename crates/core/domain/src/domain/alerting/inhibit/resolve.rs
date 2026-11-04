// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 这批事件里此刻谁被谁压着：从没人能压的事件往下走，走到的都被压。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::Serialize;

use super::{InhibitRule, match_view, may_be_target};
use crate::{
    domain::alerting::{
        incident::{Incident, Severity},
        semantic_group::{CompiledMatcher, LabelMatcher},
    },
    shared::ids::Id,
};

/// 一个事件被哪条规则、因为哪个事件压着。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inhibited {
    pub rule_id: Id,
    pub rule_name: String,
    pub source_incident_id: Id,
    pub source_summary: String,
}

/// 一条规则在一批事件上的结果：谁能当源、谁能当目标，按 `equal` 的取值分好。
struct RulePlan<'a> {
    rule: &'a InhibitRule,
    sources: Vec<bool>,
    classes: Vec<Option<Vec<String>>>,
    /// 每个 `equal` 取值上能被压的事件（下标）。
    targets: HashMap<Vec<String>, Vec<usize>>,
}

/// 这批事件里此刻谁被谁压着。`incidents` 是组织进行中的事件，`silenced` 说一个事件此刻是不是被静默。
/// 结果只取决于这批事件：每轮、每个节点算出来都一样。
pub fn resolve(
    rules: &[InhibitRule],
    incidents: &[Incident],
    silenced: impl Fn(&Incident) -> bool,
) -> HashMap<Id, Inhibited> {
    let live: Vec<&Incident> = incidents
        .iter()
        .filter(|incident| incident.is_active() && !incident.is_merged())
        .collect();

    let views: Vec<BTreeMap<String, String>> =
        live.iter().map(|incident| match_view(incident)).collect();

    let quiet: Vec<bool> = live.iter().map(|incident| silenced(incident)).collect();

    let plans: Vec<RulePlan<'_>> = rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| plan(rule, &live, &views, &quiet))
        .filter(|plan| plan.sources.iter().any(|is_source| *is_source))
        .collect();

    let mut found = HashMap::new();

    if plans.is_empty() {
        return found;
    }

    // 严重的、先开的排在前面：互相压的事件里留下它不压。
    let mut order: Vec<usize> = (0..live.len()).collect();

    order.sort_by(|left, right| {
        let (a, b) = (live[*left], live[*right]);

        b.severity
            .cmp(&a.severity)
            .then(a.created_at.0.cmp(&b.created_at.0))
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });

    let mut seen = vec![false; live.len()];
    let mut queue = VecDeque::new();

    // 没有谁能压它的事件是根：从它们走，走到的都被压。
    for &index in &order {
        if !has_possible_source(index, &plans, &live) {
            seen[index] = true;
            queue.push_back(index);
        }
    }

    walk(&mut queue, &mut seen, &mut found, &plans, &live);

    // 剩下的都在环里（或在环的下游）：留下排在最前面的不压，再走一遍，直到都走过。
    for &index in &order {
        if !seen[index] {
            seen[index] = true;
            queue.push_back(index);
            walk(&mut queue, &mut seen, &mut found, &plans, &live);
        }
    }

    found
}

fn plan<'a>(
    rule: &'a InhibitRule,
    live: &[&Incident],
    views: &[BTreeMap<String, String>],
    quiet: &[bool],
) -> RulePlan<'a> {
    let source_matchers: Vec<CompiledMatcher<'_>> = compile(&rule.source.matchers);
    let target_matchers: Vec<CompiledMatcher<'_>> = compile(&rule.target.matchers);
    let classes: Vec<Option<Vec<String>>> = views.iter().map(|view| rule.class(view)).collect();
    let mut sources = vec![false; live.len()];
    let mut targets: HashMap<Vec<String>, Vec<usize>> = HashMap::new();

    for (index, incident) in live.iter().enumerate() {
        let view = &views[index];

        let Some(class) = &classes[index] else {
            continue;
        };

        sources[index] = !quiet[index]
            && rule
                .source
                .min_severity
                .is_none_or(|floor| incident.severity >= floor)
            && source_matchers.iter().all(|matcher| matcher.matches(view));

        let target = may_be_target(incident)
            && rule
                .target
                .max_severity
                .is_none_or(|ceiling| incident.severity <= ceiling)
            && target_matchers.iter().all(|matcher| matcher.matches(view));

        if target {
            targets.entry(class.clone()).or_default().push(index);
        }
    }

    RulePlan {
        rule,
        sources,
        classes,
        targets,
    }
}

fn compile(matchers: &[LabelMatcher]) -> Vec<CompiledMatcher<'_>> {
    matchers.iter().map(LabelMatcher::compiled).collect()
}

/// 有没有哪个事件能压它。
fn has_possible_source(index: usize, plans: &[RulePlan<'_>], live: &[&Incident]) -> bool {
    plans.iter().any(|plan| {
        let Some(class) = &plan.classes[index] else {
            return false;
        };

        let is_target = plan
            .targets
            .get(class)
            .is_some_and(|targets| targets.contains(&index));

        is_target
            && plan.sources.iter().enumerate().any(|(source, is_source)| {
                *is_source
                    && source != index
                    && plan.classes[source].as_ref() == Some(class)
                    && plan
                        .rule
                        .severities_allow(live[source].severity, live[index].severity)
            })
    })
}

/// 从队列里的事件往下走：它们能压的、还没走过的事件都记成被它们压着。
fn walk(
    queue: &mut VecDeque<usize>,
    seen: &mut [bool],
    found: &mut HashMap<Id, Inhibited>,
    plans: &[RulePlan<'_>],
    live: &[&Incident],
) {
    while let Some(source) = queue.pop_front() {
        for plan in plans {
            if !plan.sources[source] {
                continue;
            }

            let Some(class) = &plan.classes[source] else {
                continue;
            };

            let Some(targets) = plan.targets.get(class) else {
                continue;
            };

            for &target in targets {
                if seen[target]
                    || target == source
                    || !plan
                        .rule
                        .severities_allow(live[source].severity, live[target].severity)
                {
                    continue;
                }

                seen[target] = true;
                queue.push_back(target);

                found.insert(
                    live[target].id.clone(),
                    Inhibited {
                        rule_id: plan.rule.id.clone(),
                        rule_name: plan.rule.name.clone(),
                        source_incident_id: live[source].id.clone(),
                        source_summary: live[source].summary.clone(),
                    },
                );
            }
        }
    }
}

/// 给规则预览的一对：`source` 压住 `target`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InhibitPair {
    pub source_incident_id: Id,
    pub source_summary: String,
    pub source_severity: Severity,
    pub target_incident_id: Id,
    pub target_summary: String,
    pub target_severity: Severity,
}

/// 这条规则（可以还没存）在这批事件上会压住哪些。别的规则不算。
pub fn preview(
    rule: &InhibitRule,
    incidents: &[Incident],
    silenced: impl Fn(&Incident) -> bool,
) -> Vec<InhibitPair> {
    let enabled = InhibitRule {
        enabled: true,
        ..rule.clone()
    };

    let found = resolve(std::slice::from_ref(&enabled), incidents, silenced);
    let by_id: HashMap<&Id, &Incident> = incidents
        .iter()
        .map(|incident| (&incident.id, incident))
        .collect();

    let mut pairs: Vec<InhibitPair> = found
        .iter()
        .filter_map(|(target_id, inhibited)| {
            let target = by_id.get(target_id)?;
            let source = by_id.get(&inhibited.source_incident_id)?;

            Some(InhibitPair {
                source_incident_id: source.id.clone(),
                source_summary: source.summary.clone(),
                source_severity: source.severity,
                target_incident_id: target.id.clone(),
                target_summary: target.summary.clone(),
                target_severity: target.severity,
            })
        })
        .collect();

    pairs.sort_by(|left, right| {
        left.source_incident_id
            .as_str()
            .cmp(right.source_incident_id.as_str())
            .then_with(|| {
                left.target_incident_id
                    .as_str()
                    .cmp(right.target_incident_id.as_str())
            })
    });

    pairs
}

/// 每条规则此刻压着几个事件。
pub fn inhibiting_counts(incidents: &[Incident]) -> HashMap<Id, usize> {
    let mut counts = HashMap::new();

    for incident in incidents.iter().filter(|incident| incident.is_active()) {
        if let Some(inhibition) = &incident.handling.inhibition {
            *counts.entry(inhibition.rule_id.clone()).or_insert(0) += 1;
        }
    }

    counts
}

/// 进行中事件的 id，给 [`settle`] 的 `is_active` 用。
pub fn active_ids(incidents: &[Incident]) -> HashSet<Id> {
    incidents
        .iter()
        .filter(|incident| incident.is_active() && !incident.is_merged())
        .map(|incident| incident.id.clone())
        .collect()
}
