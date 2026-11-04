---
name: incident-triage
description: Triage an open incident - confirm what is broken, how badly, since when, who is affected, who owns it, and what to do first.
argument-hint: <incident id or service>
version: "1"
allowed-tools: list_incidents, get_incident, get_incident_insights, get_incident_rca, list_recent_alerts, correlate_signals, get_service_topology, apm_overview, get_apm_insights, get_apm_blast_radius, compare_apm_versions, get_current_on_call
tags: [incidents, triage, on-call]
examples:
  - Triage the open incident on checkout
  - What should we do first about the incident on the public API?
---

# Incident triage

Goal: within a few tool calls, give the on-call person a decision-ready picture. Facts first, then the most likely cause, then the first safe actions. Do not stop at describing the alert.

## 1. Pin down the incident

- With an incident id, read it with `get_incident`. Otherwise list open ones with `list_incidents` and pick the one that matches the user's words; say which one you picked.
- Note the rule, severity, status, when it opened, the triggering query and its sample values, the affected services and hosts.

## 2. Measure the impact

- Use `get_incident_insights` for the incident's own context when it exists.
- Use `correlate_signals` over the incident window (15 minutes before it opened until now) for the affected services: error rate, latency, throughput, related logs and traces.
- Use `apm_overview` to see whether the problem is one service or several. Quantify: "16% of requests fail" beats "errors are high".

## 3. Find where it starts

- Use `get_apm_blast_radius` for the affected service: the services that call it are what its trouble reaches, and what it calls, the unhealthy first, is where the trouble may come from. An error that starts in a dependency and spreads upstream points to the dependency. Without APM data, `get_service_topology` gives the callers and dependencies.
- Look for a change near the start: `get_apm_insights` for the service names the moves against the stretch before and the same stretch a week before, when each began, and the deployments, configuration changes and feature flags near then; `compare_apm_versions` compares two versions, and `list_recent_alerts` finds related alerts that fired first.

## 4. Who owns it

- Use `get_current_on_call` for the escalation policy or schedule of the incident's rule, and name the person.

## 5. Answer

Write, in this order:

1. **Impact** - what users see, how much, since when (absolute time and duration).
2. **Most likely cause** - one sentence, with the evidence that supports it; say how confident you are.
3. **First actions** - safe, reversible steps first (roll back the deploy, shift traffic, scale out). Propose any change through the approval workflow; never run it directly.
4. **Owner** - who is on call.
5. **What is still unknown** - the checks that would confirm the cause.

If an existing root cause analysis is available (`get_incident_rca`), compare it with what you found and say where you agree or differ.
