#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) 2026 MoleSignal Authors
"""Seed demo metrics into a RUNNING molesignal backend (not the dev mock).

Ingests realistic, cross-month time series so the Metrics page has something to
chart over long windows:

  * cpu_usage_percent     gauge   (Float64), 3 hosts — daily/weekly seasonality + slow trend
  * memory_usage_percent  gauge   (Float64), 3 hosts — sawtooth "leak + restart"
  * http_requests_total   counter (Int64),   service x method — monotonic ramp

Metric name == stream name in molesignal; each POST auto-creates the stream.

Prerequisites:
  - The REAL backend must be running on --base (default http://localhost:5080),
    i.e. `cd molesignal && make run`. The Vite `dev:mock` server also binds 5080
    but is a stub — point this at the real backend instead.

Usage:
  python3 scripts/seed-demo-metrics.py                 # ~95 days, hourly
  python3 scripts/seed-demo-metrics.py --days 7 --step-min 5    # 1 week, 5-min
  python3 scripts/seed-demo-metrics.py --base http://localhost:5080 \
      --email admin@example.com --password admin

Note on rate(): rate(m[R]) needs >= 2 samples inside [R]. With the default hourly
step, query `rate(http_requests_total[3h])` (not [5m], which returns empty). Lower
--step-min if you need fine-grained rate over short windows.
"""
import argparse
import json
import math
import sys
import time
import urllib.request
from datetime import datetime, timezone


def main() -> int:
    ap = argparse.ArgumentParser(description="Seed demo metrics into a running molesignal backend")
    ap.add_argument("--base", default="http://localhost:5080", help="API base (real backend, not dev mock)")
    ap.add_argument("--email", default="admin@example.com")
    ap.add_argument("--password", default="admin")
    ap.add_argument("--days", type=int, default=95, help="time span in days (default 95 -> crosses months)")
    ap.add_argument("--step-min", type=int, default=60, help="point interval in minutes (default 60)")
    ap.add_argument("--chunk", type=int, default=5000, help="events per ingest POST (stay under body limit)")
    args = ap.parse_args()

    base = args.base.rstrip("/") + "/api/v1"

    def call(path, body, token=None):
        req = urllib.request.Request(base + path, data=json.dumps(body).encode(), method="POST")
        req.add_header("content-type", "application/json")
        if token:
            req.add_header("authorization", "Bearer " + token)
        with urllib.request.urlopen(req, timeout=180) as r:
            return r.status, json.loads(r.read().decode() or "{}")

    # --- login (and detect the dev mock) ---
    try:
        _, login = call("/auth/login", {"email": args.email, "password": args.password})
    except Exception as e:  # noqa: BLE001
        print(f"login failed: {e}\nIs the REAL backend running on {args.base}? (`cd molesignal && make run`)", file=sys.stderr)
        return 1
    if "org_id" not in login or "token" not in login:
        print(f"unexpected login response: {login}\n"
              f"-> {args.base} looks like the Vite dev mock, not the real backend. "
              f"Stop `dev:mock` and run `make run`.", file=sys.stderr)
        return 1
    token, org = login["token"], login["org_id"]
    print(f"logged in as {login.get('email')} (org {org})")

    step = args.step_min * 60_000_000  # micros
    now_us = int(time.time() * 1_000_000)
    n = (args.days * 24 * 60) // args.step_min

    def seasonal(ts_us):
        dt = datetime.fromtimestamp(ts_us / 1e6, tz=timezone.utc)
        daily = math.sin((dt.hour + dt.minute / 60.0 - 3) / 24 * 2 * math.pi)  # afternoon peak
        weekend = 0.6 if dt.weekday() >= 5 else 1.0
        return daily, weekend

    cpu, mem, http = [], [], []
    cpu_base = {"web-1": 34.0, "web-2": 48.0, "db-1": 61.0}
    mem_base = {"web-1": 52.0, "web-2": 58.0, "db-1": 71.0}
    series = [("checkout", "GET"), ("checkout", "POST"), ("api", "GET"), ("api", "POST")]
    counters = {s: 1_000_000 + i * 250_000 for i, s in enumerate(series)}
    leak_period = max(1, (24 * 5 * 60) // args.step_min)  # ~5 day sawtooth in steps

    for i in range(n, 0, -1):
        ts = now_us - i * step
        daily, weekend = seasonal(ts)
        trend = (n - i) / n
        for host, b in cpu_base.items():
            v = b + 13 * daily * weekend + 8 * trend + 3 * math.sin(i / (leak_period))
            cpu.append({"_timestamp": ts, "value": round(max(3.0, min(96.0, v)), 1), "host": host})
        for host, b in mem_base.items():
            leak = (i % leak_period) / leak_period * 22.0
            v = b + (22.0 - leak) + 4 * daily
            mem.append({"_timestamp": ts, "value": round(max(10.0, min(97.0, v)), 1), "host": host})
        for s in series:
            svc, method = s
            inc = (900 + 700 * max(0.0, daily)) * weekend * (0.8 + 0.6 * trend)
            counters[s] += int(inc) + (hash((svc, method, i)) % 80)
            http.append({"_timestamp": ts, "value": int(counters[s]), "service": svc, "method": method})

    def post_chunked(metric, events):
        total = 0
        for k in range(0, len(events), args.chunk):
            st, r = call(f"/ingest/metrics/{metric}", events[k:k + args.chunk], token)
            if st != 200:
                print(f"ingest {metric} failed: {st} {r}", file=sys.stderr)
                return False
            total += r.get("accepted", 0)
        print(f"  {metric}: {total} points")
        return True

    print(f"ingesting {args.days} days @ {args.step_min}-min step ({n} points/series)...")
    ok = post_chunked("cpu_usage_percent", cpu) and post_chunked("memory_usage_percent", mem) and post_chunked("http_requests_total", http)
    if not ok:
        return 1

    lo = datetime.fromtimestamp((now_us - n * step) / 1e6, tz=timezone.utc).date()
    hi = datetime.fromtimestamp(now_us / 1e6, tz=timezone.utc).date()
    print(f"done. span {lo} -> {hi}")
    print("query examples (set the Metrics time window to cover the span):")
    print("  cpu_usage_percent")
    print("  memory_usage_percent")
    print("  http_requests_total                 # raw counter ramp")
    print(f"  rate(http_requests_total[{max(3, args.step_min // 20)}h])   # rate; range >= data step")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
