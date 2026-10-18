---
name: latency-regression
description: Explain a latency regression - from APM to slow traces to dependencies, then compare versions and CPU profiles before and after.
argument-hint: <service> [endpoint]
version: "1"
allowed-tools: apm_overview, get_apm_service, list_apm_transactions, get_apm_transaction, list_apm_dependencies, list_traces, get_trace, get_trace_dag, compare_apm_versions, list_continuous_profiles, compare_profiles, get_profile_flamegraph
tags: [apm, latency, performance]
examples:
  - Checkout p95 doubled since this morning, why?
  - Find what made the search endpoint slow
---

# Latency regression

Goal: name the code path or dependency that got slower, when, and why, with numbers from before and after.

## 1. Confirm and locate

- `get_apm_service` for the service: p50, p95, p99, throughput and error rate now against an earlier healthy window of the same length.
- `list_apm_transactions` to find the endpoints whose latency grew the most. Work on the one with the largest change weighted by traffic.

## 2. Slow traces

- `list_traces` for that endpoint, slowest first, inside the regression window. Open two or three with `get_trace` or `get_trace_dag` and find the span that holds most of the time.
- Compare with a fast trace of the same endpoint from before: is it the same span getting slower, or a new span (an extra call, a retry, a lock)?

## 3. Dependencies

- `list_apm_dependencies` for the service: a database, cache or downstream service whose latency grew at the same time is the likely carrier. Follow it one level down if it is ours.

## 4. What changed

- `compare_apm_versions` to see whether a new version started serving when the latency moved.
- With continuous profiles available, `compare_profiles` before and after the change for the service, and read the frames whose CPU share grew with `get_profile_flamegraph`.

## 5. Answer

State the endpoint and the size of the regression (before vs after, p95 and p99), when it started, the span or dependency that carries it, the version or change that lines up, and the code path the profile points at. End with the cheapest way to confirm (one targeted query or a rollback on one instance).
