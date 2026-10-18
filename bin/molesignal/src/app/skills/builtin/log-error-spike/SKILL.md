---
name: log-error-spike
description: Investigate a spike of errors in logs - when it started, which messages and services it comes from, and what changed at that moment.
argument-hint: <service or stream> [time range]
version: "1"
allowed-tools: list_streams, get_stream_schema, query_logs, search_field_values, list_log_patterns, get_log_pattern, search_around, list_traces, get_trace, compare_apm_versions
tags: [logs, errors]
examples:
  - Why did checkout start logging so many errors?
  - Find what changed when the payment errors jumped
---

# Log error spike

Goal: explain a rise in error logs with the first occurrence, the messages behind it, and the change that lines up with it.

## 1. Find the right stream and fields

- `list_streams`, then `get_stream_schema` for the stream of the service. Use only fields that exist: find the level, service and message fields by their names in the schema.
- If the user did not give a time range, look at the last 6 hours and widen to 24 hours if the spike is older.

## 2. Shape of the spike

- With `query_logs`, count error-level events per 5-minute bucket for the service, and compare with the same window a day earlier. Report when the count left its normal range, not when an alert fired.

## 3. What the errors are

- Use `list_log_patterns` (or group by the message field) to find the top error patterns inside the spike, with counts. Separate new patterns (absent before the spike) from old ones that only grew.
- For the top new pattern, read a few raw events with `search_around` to see the surrounding context: request ids, upstream hosts, exception class.

## 4. Where they come from

- Use `search_field_values` on host, pod, version or endpoint fields inside the spike. A spike limited to one host, zone or version narrows the cause.
- When events carry trace ids, open one with `get_trace` to see which call fails.

## 5. What changed

- `compare_apm_versions` for a deploy close to the start. Mention config or dependency changes when the logs show them.

## 6. Answer

Give the start time, the size of the spike against the baseline, the two or three patterns that make it up with counts, where they come from, the change that lines up with it, and the next check that would confirm the cause. Quote one representative log line per pattern, shortened.
