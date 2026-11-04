---
name: rum-triage
description: Triage a problem real users have - from the overview of a RUM application to the issues behind it, the sessions that met them and the traces of their requests.
argument-hint: <application> [environment] [time range]
version: "1"
allowed-tools: get_rum_overview, list_rum_issues, get_rum_issue, get_rum_vitals, get_rum_frustration, get_rum_view_waterfall, aggregate_rum, get_rum_funnel, get_rum_paths, get_rum_retention, list_rum_sessions, get_rum_session, list_rum_actions, list_rum_errors, get_rum_related_traces, get_trace, get_trace_dag, compare_apm_versions
tags: [rum, frontend, errors, performance]
examples:
  - The checkout page of shop-web got slower after yesterday's release, why?
  - Users say the app hangs when they pay, what are they hitting?
---

# RUM triage

Goal: say what real users of the application are hitting, how many of them, since when, and the likeliest cause, with evidence the reader can open.

## 1. Overview first

- `get_rum_overview` for the application (and the environment, if the question names one) over the range the question names; the last 24 hours when it names none, wider if the problem is older. It gives the sessions, the share without an error or a crash, the frustrated sessions, the three Core Web Vitals with their ratings, the pages most viewed and the unresolved issues touching the most sessions.
- Say what is out of the ordinary before going deeper, and if nothing is, say that. A move needs a baseline: read the same overview over the stretch before, or over a healthy day the question names, and compare.

## 2. Issues

- `list_rum_issues` (unresolved, ranked by the sessions each touched). Use `fresh: true` when the problem began recently: it keeps the issues first seen in the range or regressed in it. Read the trend: a rise at the moment the problem began matters more than a big old issue.
- `get_rum_issue` for the one or two that line up: the version it began in, whether it regressed, the stack, the latest sessions it happened in.
- For errors that belong to no issue, or to see where they concentrate, use `aggregate_rum` over `errors` grouped by `version`, `view.url_path_group`, `browser.name` or `device.type`; `list_rum_errors` for the rows.

## 3. Slow or stuck

- Slow pages: `get_rum_vitals` page by page, slowest first. `aggregate_rum` over `views` gives the 75th percentile by `version`, `browser.name`, `device.type` or `geo.country_iso_code`, and says where the slowness concentrates. Durations are nanoseconds.
- `get_rum_view_waterfall` for a view that was slow (take its id from `list_rum_actions` with `stream: views`, or from the sessions of an issue): what the largest paint waited for, the resources that were slow, blocked rendering or failed, and the long tasks. Look at the slowest resources and at the host they come from.
- `get_rum_frustration` for rage, dead and error clicks by element, page or version: where users fight the page.

## 4. Journeys

- `get_rum_funnel` when the problem is a flow (a checkout, a sign-up): the steps are pages (`views`, told by `view.url_path_group`) or things people did (`actions`), in the order they are taken. It says how many sessions got through each step, the share of the first step's, and how long they took. Break it down by `version`, `browser.name`, `os.name` or `device.type` to see who falls out. The step where most are lost is where to look next: its issues, its frustration and its loading.
- `get_rum_paths` when the loss is a detour, not a step: where sessions went after a page (`direction: after`), or came from before it (`before`).
- `get_rum_retention` when the question is whether people come back after a change: the people first seen in a day or a week and the share seen again in each period after. A period that has not ended says nothing.

## 5. Sessions

- `get_rum_session` for one or two affected sessions, from an issue or from the frustration table: its views, actions and errors in the order they happened, and whether a replay exists. One session is an example, not a count: say how many sessions you know are affected from the aggregates.

## 6. Traces

- `get_rum_related_traces` for a session with a failing or slow request: the requests carry a trace id. Open the trace with `get_trace` or `get_trace_dag` and name the backend span that failed or waited. `compare_apm_versions` when a release of the backend lines up with the start.

## 7. Answer

State what users see and how many (sessions, share, versions, pages, browsers), since when, the likely causes ranked with the evidence for each, what could not be confirmed (for example, only Chromium reports the element of the largest paint; a session without a user id cannot be followed to a person), and the next step that would confirm the cause.

Cite the evidence with routes of the product, so the reader can open it. Leave `env` out when the question covers every environment.

- the overview: `/rum/overview?app=<application>&env=<environment>`
- an issue: `/rum/errors?app=<application>&env=<environment>&issue=<issue id>`
- a session: `/rum/sessions/view/<session id>`
- the loading of a slow view: `/rum/performance?app=<application>&env=<environment>&view=<view id>&at=<start of the view in microseconds>`
- frustration: `/rum/frustration?app=<application>&env=<environment>`
- a funnel: `/rum/analytics?app=<application>&env=<environment>&steps=v:view.url_path_group=%2Fcart|v:view.url_path_group=%2Fcheckout|a:action.target.name=Pay%20now` (`v:` is a page, `a:` an action, the value is URL-encoded); `&by=browser.name` counts the sessions by a field
- the paths from a page: `/rum/analytics/paths?app=<application>&env=<environment>&pivot=<page>` (`&dir=before` for the pages before it); the retention: `/rum/analytics/retention?app=<application>&env=<environment>`
- the rows behind a number: `/rum/explorer?type=<sessions|views|actions|errors|resources>&q=<field = 'value' AND field = 'value'>`
