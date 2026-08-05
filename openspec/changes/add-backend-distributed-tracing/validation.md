# Validation Record

Date: 2026-07-27

## Passed

| Command or coverage | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed. The stable toolchain only reports that `imports_granularity` and `group_imports` are nightly-only options. |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | Passed without lint allowances. |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed without lint allowances after resolving two `js-runtime`-only `collapsible_if` diagnostics. |
| `cargo test --locked --offline` | Passed: 931 library tests plus all enabled integration and documentation targets. |
| `cargo test --all-features --locked infra::runtime::js_executor::tests` | Passed: 4 JS runtime tests; unrelated test targets were filtered out. |
| `cargo test -p sqlx` | Passed: 5 tests, including SQL HMAC fingerprint properties. |
| Focused Trace suites | Passed: Canonical normalization/sanitization, self telemetry, tail sampling, candidate routing, fan-out/export, propagation, service graph, coverage inventory, permissions, and config tests. |
| Mock OTLP collectors | Passed for gRPC and HTTP/protobuf, including metadata/auth, gzip, TLS/mTLS validation, matching retained sets, failure isolation, loop rejection, shutdown, and hostile-value redaction. |
| `pnpm -C web exec tsc --noEmit` | Passed with the updated Trace flame-chart fixture contract. |
| `ruby -e 'require "yaml"; YAML.load_file("docs/api/openapi.yaml")'` | Passed. |
| `openspec validate add-backend-distributed-tracing --strict` | Passed. |
| `scripts/check_distributed_tracing_overhead.sh` | Passed with the representative default workload and 40 samples: CPU `566.418 ms -> 575.735 ms` (`+1.64%`, limit `5%`); P95 `14.951125 ms -> 15.023875 ms` (`+0.49%`, limit `3%`). |

The performance workload covers 100,000 ingestion events, 100,000 PromQL samples, and an
8 MiB object-store put/get cycle. An earlier tiny synthetic workload incorrectly amplified the
fixed per-operation cost and reported CPU `+9.37%` and P95 `+16.36%`; the benchmark was corrected
to the representative workload before applying the release thresholds.

### Fixed `_sys` target follow-up

The development-stage configurable system-organization slug and its compatibility path were
removed. `SYSTEM_ORG_SLUG` is now the sole runtime target source, and a configuration containing
the removed field fails as unknown. The following focused checks passed:

- `cargo test --locked --offline --lib config::tests` (25 tests)
- `cargo test --locked --offline --test config_parse_default_conf` (2 tests)
- `cargo test --locked --offline --lib bootstrap::wire::tests::self_telemetry` (3 tests)
- `cargo clippy --all-targets --locked --offline -- -D warnings`
- strict validation for both `add-backend-distributed-tracing` and `ingest-self-telemetry`

## All-Features Gate

After explicit authorization, Cargo downloaded the missing optional dependency graph, including
`az 1.3.0`, and executed the required third-party build scripts and procedural macros. The first
all-features Clippy run exposed two `collapsible_if` diagnostics in the optional JS runtime. After
the equivalent control-flow cleanup, the exact release gate passed and the four focused JS runtime
tests passed.

## Environment-Gated Coverage

- `tests/bootstrap_it_distributed_tracing.rs` compiles and its normal gated path passes. The
  `MS_RUN_IT=1` branch could not execute because `/var/run/docker.sock` is unavailable on this host.
- The Docker-backed intelligence-chat test, Chrome-backed scheduled PDF/PNG tests, PromQL
  performance smoke test, distributed-tracing debug performance test, and one documentation test
  remain intentionally ignored by their existing platform/tooling gates.
- Mock-collector tests require local loopback sockets; they passed when run with the required
  sandbox permission.
