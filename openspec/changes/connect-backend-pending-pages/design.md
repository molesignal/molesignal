## Context

The frontend already has many API clients for settings, RUM, scheduled reports, scheduled pipelines, and running queries. Several pages still use backend-pending copy as their empty state because those routes were built before the corresponding backend endpoints landed. Some RUM pages also call the generic query endpoint but still label empty results as backend-pending.

## Goals / Non-Goals

**Goals:**
- Reserve backend-pending UI only for true missing endpoints.
- Connect implemented backend endpoints to their frontend pages with normal loading, error, and empty states.
- Prefer existing generic `/query` access for RUM telemetry rather than adding duplicate read endpoints in this pass.
- Keep page-level changes small and compatible with the current product templates.

**Non-Goals:**
- Implement full billing, SaaS usage, invitations workflow, or a new RUM aggregation service.
- Change storage schemas unless a minimal read endpoint can use already-existing state.
- Redesign the settings, IAM, or RUM visual layout.

## Decisions

- Use existing endpoints first. Settings pages backed by `/license`, `/model_prices`, `/notify/templates`, `/regex_patterns`, `/ai_toolsets`, `/query/running`, `/scheduled_reports`, and `/scheduled_pipelines` will keep their current clients and remove backend-pending empty variants.
- Treat empty arrays as real empty states. A successful `[]` response means "configured but no rows yet", not "backend missing".
- Keep RUM query-backed. The backend currently exposes RUM ingest endpoints and stores events in streams; frontend read pages will continue to query `rum_sessions`, `rum_errors`, and `rum_actions` through `/query`.
- Defer truly missing product workflows. Invitations and richer service-account creation require durable invite/account semantics and token display rules; they remain out of this first implementation unless a safe derived view exists.

## Risks / Trade-offs

- [Risk] Some endpoints are -gated and return 403 in OSS. -> Mitigation: render a FeatureGate/permission-aware error instead of backend-pending where the endpoint exists.
- [Risk] Empty settings pages could feel less informative after removing backend-pending copy. -> Mitigation: use feature-specific empty titles/descriptions and keep primary create actions visible.
- [Risk] RUM SQL may differ from final dedicated endpoints. -> Mitigation: isolate mapping in `web/src/api/rum.ts` so a future endpoint swap is local.
