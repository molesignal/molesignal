#!/usr/bin/env bash
set -euo pipefail

export MS_RUN_TRACE_PERF=1
export MS_TRACE_PERF_SAMPLES="${MS_TRACE_PERF_SAMPLES:-40}"

cargo test --release --test perf_distributed_tracing \
  default_trace_capture_stays_within_cpu_and_p95_budgets -- --ignored --nocapture
