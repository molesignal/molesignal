# Security Policy

> 中文版本 / Chinese version: [SECURITY.zh-CN.md](SECURITY.zh-CN.md)

Thanks for taking the time to look at MoleSignal's security posture. The project is pre-1.0 and self-hosted; many of the features it ships (multi-tenant query rewriting, cipher-key envelope encryption, JWT signing-secret rotation, audit log, ingest quotas) are explicitly security-sensitive, so we treat reports seriously.

## Supported Versions

While MoleSignal is pre-1.0, only the `main` branch receives security fixes. Backports to older tags are best-effort.

| Version           | Supported |
|-------------------|-----------|
| `main` (HEAD)     | ✅        |
| Latest tagged release on `main` | ✅ |
| `beta` / `alpha` channels       | ✅ (forward fixes only) |
| Older tags        | ❌        |

After 1.0 ships, we will revisit this table and document an LTS policy.

## Reporting a Vulnerability

**Please do not file a public GitHub issue.** Pick one of the two private channels below:

- **GitHub private vulnerability reporting**: <https://github.com/molesignal/molesignal/security/advisories/new>
- **Email**: <security@molesignal.io> (PGP key on request)

Include:

- A short description of the issue and the impact you observed.
- A minimal reproduction (config snippet, request payload, command sequence).
- The commit SHA (or release tag) you tested against.
- Your suggested severity and any mitigations you have in mind.

## Scope

In scope:

- The MoleSignal server (`crates/bootstrap`, `crates/api`, `crates/app`, `crates/infra`, `crates/domain`, `crates/shared`).
- The web client under `web/`.
- The reference Docker images and Compose / Kubernetes manifests under `deploy/`.
- Cross-tenant data leaks, authentication / authorisation bypass, signing-secret / cipher-key exposure, ingest-side injection, and any deviation between the documented multi-tenant isolation guarantees and observed behaviour.

Out of scope (please don't report these as vulnerabilities):

- DoS that requires an unrealistic config (e.g. you tuned `[wal]` flush to disable backpressure and then flooded the API).
- Findings in dependencies that are not reachable from MoleSignal's code paths — please report those upstream.
- Issues that only affect non-default builds you've patched yourself.
- Missing security hardening features that are tracked in the public roadmap.

## What to Expect

- **Acknowledgement** within **3 business days** of submission.
- **Initial triage** (confirmed / not-a-vuln / need-more-info) within **7 business days**.
- **Fix or mitigation plan** within **30 days** for high/critical issues, longer for low-severity ones, communicated through the advisory.
- We coordinate disclosure: once a fix is shipped on `main` and a release tag, we publish the advisory and credit you in the changelog unless you ask to stay anonymous.

We are a small team — please be patient if it takes a bit longer than the windows above. If you do not hear back, ping the advisory thread.

## Hardening Reminders for Operators

If you run MoleSignal in production, the following knobs are not optional:

- **Set `MS_CIPHER_KEY`** to a real 32-byte base64 secret (the all-zero fallback is dev-only and logged at WARN).
- **Rotate the bootstrap JWT signing secret** (`POST /api/v1/auth/jwt/rotate`) after first start.
- **Restrict `/api/v1/_*` and `/metrics`** at the ingress layer to internal callers — they expose admin-grade surface.
- **Run the planner-rewrite tests in your fork** when you touch query code; the `it_multitenant.rs` suite is the contract that keeps tenants isolated.
- **Enable per-org quotas** if your ingest is shared — runaway producers should hit 413/429 before they degrade neighbours.

Issues found while hardening a deployment are exactly the reports we want.

## Acknowledgements

Reporters who follow this process and want credit will be listed in the release advisory and in `CHANGELOG.md` for the release that contains the fix.
