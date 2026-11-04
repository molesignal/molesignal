# RUM event contract

This directory is the language-neutral source of truth for the events that SDKs send to `POST /api/v1/rum/events`.

## Ownership

- `v1.schema.json` (JSON Schema, draft 2020-12) describes one event. The request body is a JSON array of events, or a single event, or NDJSON.
- `fixtures/valid/` and `fixtures/invalid/` are shared by the server tests and by the SDK repositories (`browser-rum`, `flutter-rum`). `fixtures/manifest.json` lists every case, whether it must pass, the issue codes an invalid one must produce, and which fixtures are batches (`"batch": true`: a JSON array whose items are each validated).
- The server validates every event against this schema when it arrives (`crates/core/contracts`), so a bad event is rejected on its own and the rest of the request is kept. The field table that flattens an event into stream columns lives in `crates/core/domain/src/domain/rum/event/`; a test keeps it in step with this schema.

## Rules that the schema cannot state

- `date` is epoch milliseconds. For a `view` it is the start of the view and **must not change between revisions**: the compactor keeps the highest revision of a view only among rows that share the same `date`.
- Every duration, and every time relative to the start of a view (`performance.lcp.timestamp`, `resource.dns.start`, …), is an **integer number of nanoseconds**. Round before sending.
- `session.start` (epoch milliseconds, optional but expected) is when the session began. A node that does not know a session yet looks at it: a session that began moments ago is new, an older one is looked up in what was already written.
- A view is sent again, with a higher `view.revision` and its counts so far, whenever something in it changed; the server keeps the highest revision. The counts are cumulative, never deltas. Sessions are derived by the server from the views; an SDK sends no session event.
- The application is the one the client token is bound to. `application.id` is optional and a different value is rejected.
- `geo` and `network` are written by the server and ignored when sent. For browser events the server also fills `browser`, `os` and `device` from the `User-Agent` when the event does not carry them, and adds `view.url_host`, `view.url_path` and `view.url_path_group` (and the same for `resource.url`). For a long task it adds `long_task.top_script.source_url`, `long_task.top_script.function` and `long_task.top_script.invoker_type` from the entry of `long_task.scripts` that ran the longest (`duration`); what a client sends in those columns is dropped.
- A `log` event says its `level` and `message` at the top of the event, not under `log`, so that a Logs stream reads them as it reads any log. It belongs to the session and view it was written in, like every other event, and is not counted on the view.
- `session.type` is `user` unless a test drove the session. A session of type `synthetics` names the check in `synthetics` and one of type `ci_test` the test run in `ci_test`. The figures of an application leave both out unless a reader asks for them, the errors of such a session open no issue, and such a session is not sampled away unless a retention filter of the application names `session.type`.
- `action.position` is where in the window a click landed, `action.page_position` where on the page, and `action.target.offset` where in the element. A heatmap goes by the last, so that a click lands on the same element whatever the size of the window.
- Fields the schema does not know are accepted and kept (`extra`); a field that the schema knows with the wrong type or value rejects the event.
- `error.frames` carries the structured stack that source maps, ProGuard/R8 mappings and native symbol files are applied to; `error.stack` is the same stack as text for display.
- A frame without `artifact_kind` that has a `file` and a `line` is a JavaScript frame (`javascript_sourcemap` says the same): the source map is found by `debug_id`, or else by the file name. A native frame names its `artifact_kind`; the debug artifact is found by the frame's `debug_id` (the build identifier it was uploaded under), then by its `build_id`. The platform comes from `os.name`, the architecture from `device.architecture`, and the build identifier of an event from its `build_id`.

## Changing the contract

Adding an optional field, an enum value or a new event type is compatible: add it to the schema, the field table and a valid fixture in the same change. Anything else (renaming, changing a unit, making a field required) needs every SDK repository updated first. Do not change a published fixture to make a test pass; add a new one.
