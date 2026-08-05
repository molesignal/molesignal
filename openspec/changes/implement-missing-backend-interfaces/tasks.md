## 1. Backend Routes

- [x] 1.1 Implement alert rule, escalation policy, and notify channel create/update handlers.
- [x] 1.2 Implement schedule create/update and override add/remove handlers.
- [x] 1.3 Implement enrichment table list/table/upsert/delete routes backed by `enrichment_kv`.
- [x] 1.4 Implement IAM invitation repository, migration, routes, and app wiring.
- [x] 1.5 Implement correlation provider list and report template catalog routes.
- [x] 1.6 Implement function dry-run route for VRL samples.
- [x] 1.7 Implement audit activity and IAM role matrix read routes used by remaining backend-pending UI.

## 2. Frontend Wiring

- [x] 2.1 Replace remaining backend-pending UI for enrichment tables, invitations, correlation providers, report templates, alerts, and function dry-run with live calls.
- [x] 2.2 Update i18n copy to describe actual empty states and unsupported delivery-only limitations.

## 3. QA

- [x] 3.1 Run focused Rust tests/checks for touched crates.
- [x] 3.2 Run frontend typecheck, targeted lint, and build.
- [x] 3.3 Run OpenSpec validation for `implement-missing-backend-interfaces`.
