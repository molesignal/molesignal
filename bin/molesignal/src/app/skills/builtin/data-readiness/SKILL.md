---
name: data-readiness
description: Check whether the telemetry here is ready for investigations - one name per service across logs, metrics and traces, trace ids in logs, one spelling per environment, metric units - and list the fixes in priority order.
argument-hint: "[streams or services to check]"
version: "1"
allowed-tools: check_data_readiness, todo_write, list_streams, get_stream_schema, search_field_values, list_metric_names, list_metric_labels, search_service_catalog, list_functions, get_function, test_function, create_function
tags: [data quality, onboarding]
examples:
  - Is our data ready for Mole Agent to investigate incidents?
  - Why can't our logs be matched to traces?
  - Do logs, metrics and traces call our services by the same names?
---

# Data readiness

Goal: tell what in the organization's telemetry slows an investigation down or sends it the wrong way, and how to fix each thing, most important first. A person new to these systems should be able to follow the same data from an alert to a log to a trace; the check finds where that breaks.

Keep a task list with `todo_write` when the report covers more than a few streams.

## 1. Run the check

- `check_data_readiness` over the streams the question names, or over every stream when it names none; the last 24 hours unless the question asks for another range. When the question names services rather than streams, find their streams first with `list_streams` and `search_field_values` on the service field.
- Read the summary first: how many findings are critical, warnings and information, and which streams could not be read. Say when the check left streams out, and offer to check those next.

## 2. Confirm before you report

- A finding rests on field names and samples. Where one looks wrong (the service is in a field the check does not know, an environment value that is really a region), confirm it with `get_stream_schema` or `search_field_values` and drop or correct it.
- For a service written in several ways, look it up with `search_service_catalog`: aliases recorded there are known spellings, and a name missing from the catalog is worth adding.
- For metrics without units, `list_metric_labels` on one or two of them shows whether the unit is said elsewhere (a `unit` label).

## 3. Report

Write it for the people who own the sources:

- **Ready**: the streams and signals that already record the service, the environment and the trace id consistently. Keep this short.
- **What blocks investigations**, grouped: naming (a service or a key written several ways), correlation (logs without trace ids, services seen in one signal only), structure (JSON left as text, messages with nothing beside them, times without a zone), units. For each: which streams, how much of the data, and what an investigation cannot do because of it.
- **Fixes in priority order**: critical first, then what affects the most data. Say where each fix belongs: at the source (an OpenTelemetry resource attribute, the logging library) or in the stream's pipeline (a function), and give the exact change.
- **What to record until it is fixed**: spellings that cannot change soon, written as aliases in the service catalog or as a mapping in the knowledge (MOLE.md), so investigations know that `order-api` in logs is `order_service` in metrics.

## 4. Offer the pipeline functions

- When a fix is a renaming or a mapping, offer to draft the function. Write it in VRL, read one or two real rows of the stream, and run the function on them with `test_function`; show the rows before and after.
- Propose it with `create_function` only after the person says so. The proposal waits for approval like any change; once it is created, it still has to be attached to the stream's pipeline on the Pipelines page, which the person does.
- Never propose deleting or rewriting data that is already stored: the fixes apply to data from now on.
