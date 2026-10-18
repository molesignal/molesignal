---
name: alert-noise-reduction
description: Find the alert rules that page people without need - flapping, duplicated, never acted on - and propose concrete changes for review.
argument-hint: [time range, default 7d]
version: "1"
allowed-tools: list_alert_rules, get_alert_rule, list_incidents, get_incident, list_incident_groups, list_mute_rules, list_escalation_policies, list_notification_deliveries, test_alert_rule
tags: [alerting, noise, on-call]
examples:
  - Which alerts woke people up for nothing last week?
  - Help me reduce alert noise for the payments team
---

# Alert noise reduction

Goal: a short, ranked list of rule changes that remove pages nobody needed, each backed by numbers. Propose changes; do not change rules directly.

## 1. Collect the history

- `list_incidents` for the window (default the last 7 days, all statuses) and group them by rule. For each rule: number of incidents, median time to resolve, how many resolved by themselves within minutes, how many were acknowledged.
- `list_notification_deliveries` to see how many notifications each rule sent and to whom.

## 2. Spot the noise

Look for, in this order:

1. **Flapping** - incidents that open and resolve within a few minutes, many times a day. Usually the threshold sits at normal variance or the evaluation window is too short.
2. **Never acted on** - incidents resolved without acknowledgement, or acknowledged hours later without follow-up.
3. **Duplicates** - several rules firing together for one cause; `list_incident_groups` shows which already group, and which could.
4. **Out of hours on non-production** - staging or batch jobs paging at night; check `list_mute_rules` for existing windows.

## 3. Check the rule

- `get_alert_rule` for each candidate: query, threshold, duration, severity, labels. Use `test_alert_rule` to see how often a proposed threshold or duration would have fired over the same window.

## 4. Answer

A table: rule, incidents in the window, pages, the pattern you saw, the proposed change (raise the threshold to X, require N minutes, group by service, mute non-production at night, lower the severity), and the expected number of incidents after the change. Propose each change through the approval workflow with `propose_alert_action` when the user asks for it.
