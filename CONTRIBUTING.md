# Contributing to MoleSignal

> 中文版本 / Chinese version: [CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)

Thanks for considering a contribution! This guide covers how to get a dev environment running, the conventions the code follows, and the PR process.

If you are reporting a vulnerability, please follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Table of contents

- [Where to start](#where-to-start)
- [Developer Certificate of Origin (DCO)](#developer-certificate-of-origin-dco)
- [Development setup](#development-setup)
- [Coding conventions](#coding-conventions)
- [Tests](#tests)
- [Commit messages](#commit-messages)
- [Branching & release channels](#branching--release-channels)
- [Pull request process](#pull-request-process)
- [Getting help](#getting-help)

## Where to start

- New contributors: scan the [README](README.md) and skim the [ARCHITECTURE.md](ARCHITECTURE.md) — the DDD layering and the cross-signal correlation idea are load-bearing for most changes.
- In-flight design work lives under [`openspec/changes/`](openspec/changes); each change has a `tasks.md` you can pick a checked-out item from.
- The README's "Status" table shows where we want help most (production hardening, demo dataset, real-world cross-signal correlation use cases).
- Good first issues are labelled `good first issue` on GitHub when available; otherwise small typo fixes and doc improvements are always welcome.

## Developer Certificate of Origin (DCO)

We use the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) to keep the provenance chain clean. Every commit must carry a `Signed-off-by:` trailer asserting you wrote the patch (or have the right to submit it) under the project license.

Add it automatically with:

```bash
git commit -s -m "your message"
```

If you forget on an existing commit, amend with `git commit --amend -s`. For a series of commits use `git rebase --signoff <base>`.

We don't require a CLA — the DCO trailer plus the Apache-2.0 licence header at the top of every source file is enough.

## Development setup

Prerequisites:

- Rust toolchain pinned by [`rust-toolchain.toml`](rust-toolchain.toml). `rustup` picks it up automatically.
- A nightly rustfmt for the `imports_granularity` / `group_imports` options:
  `rustup toolchain install nightly --profile minimal --component rustfmt`
- `protoc` (e.g. `apt-get install protobuf-compiler` or `brew install protobuf`) and the [Buf CLI](https://buf.build/docs/installation) for the gRPC bindings.
- Docker (for integration tests and the sandbox compose stack).
- Node 20 + `pnpm` 9 if you touch `web/`.

Quick sanity loop:

```bash
make proto                                          # generate gRPC code
cargo +nightly fmt --all                            # match the rustfmt config
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib --bins                 # fast: unit + bin tests only

# Sandbox: Postgres + MinIO + molesignal standalone
docker compose -f deploy/docker/docker-compose.yaml --profile standalone up
```

Web client:

```bash
pnpm -C web install --frozen-lockfile
pnpm -C web typecheck && pnpm -C web lint
pnpm -C web test:run
pnpm -C web dev          # vite dev server
```

A pre-commit hook is installed via `make install-hooks` and enforces the license header, nightly fmt, and clippy locally — please don't bypass it with `--no-verify`.

## Coding conventions

- **DDD layering.** Keep dependency arrows pointing inward: `bootstrap → api → app → domain → shared`. Infra (`crates/infra`) implements `domain` ports, never the other way around.
- **No premature abstraction.** Three similar lines is better than a generic helper used twice.
- **Comments only when the *why* is non-obvious.** Well-named identifiers explain *what*; comments should capture invariants, workarounds, or surprising constraints.
- **No backwards-compat shims** unless we explicitly need them; deleted code stays deleted.
- **Public types get one sentence** explaining why they exist. Internal helpers don't need that.
- **License headers** — every Rust / TS source file starts with the SPDX banner enforced by the `licensure` hook.

## Tests

- Unit tests live next to the code they test (`#[cfg(test)] mod tests`).
- Integration tests live in `crates/*/tests/it_*.rs`.
- Anything that needs Docker (Postgres testcontainer, MinIO, Pebble, …) goes behind `MS_RUN_IT=1`:

  ```rust
  if common::skip_unless_enabled() { return; }
  ```

  Run the full it suite with:

  ```bash
  MS_RUN_IT=1 cargo test -p molesignal-bootstrap --tests -- --test-threads=1
  ```

- For UI / frontend work, exercise the change in a browser before claiming done — type checks and unit tests do not catch UX regressions.
- If you touch query planning or multi-tenant code, `crates/bootstrap/tests/it_multitenant.rs` and `it_planner_rewrite.rs` are the contract you must keep green.

## Commit messages

- Subject ≤ 72 chars, imperative mood (`fix(api): …`, `feat(query): …`, `refactor(infra): …`).
- Conventional Commits prefixes are preferred (`feat`, `fix`, `refactor`, `docs`, `test`, `ci`, `chore`).
- Focus the body on *why*, not *what*. The diff already shows what.
- Write subject, body, and trailers in English. (The conversational language in issues / PR comments can be either Chinese or English.)
- Every commit must carry `Signed-off-by:` (see [DCO](#developer-certificate-of-origin-dco)).

## Branching & release channels

We have four release channels driven by branches. Every channel promotes the same Cargo `release` artifact; deployment sets the runtime `RELEASE_CHANNEL` metadata:

| Branch  | Channel | Tagged as          | Promoted from |
|---------|---------|--------------------|---------------|
| `alpha` | alpha   | `vX.Y.Z`           | feature PRs   |
| `beta`  | beta    | `vX.Y.Z`           | `alpha`       |
| `rc`    | rc      | `vX.Y.Z`           | `beta`        |
| `main`  | stable  | `vX.Y.Z`           | `rc`          |

`v*` tags carry the *same* semver across channels; the workflow infers the channel from the branch the tag sits on (`main > rc > beta > alpha` if a commit reaches multiple). See [`.github/workflows/release.yml`](.github/workflows/release.yml).

Builds are identified by their Git SHA and CI `BUILD_ID`. Promotion must reuse the immutable binary or image for that build ID rather than recompiling it.

Day-to-day PRs target `alpha` (or `main` if there is no `alpha` branch yet — until promotion process is set up).

## Pull request process

1. Open an issue or discussion first for non-trivial changes — we'd rather align on direction before you spend a weekend on a refactor.
2. Branch off the target channel, keep PRs small and focused. One logical change per PR.
3. Update relevant docs (`README.md`, `ARCHITECTURE.md`, in-crate doc comments) when you change observable behaviour.
4. Make sure the CI required jobs pass locally before pushing:
   - `cargo +nightly fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --lib --bins`
   - For features that touch HTTP / wire / persistence: the relevant `it_*.rs` suite under `crates/bootstrap/tests/` with `MS_RUN_IT=1`.
5. Push, open the PR, and fill in the template. Include:
   - Motivation (what problem this solves; which user-facing behaviour changes).
   - Test plan (what you ran locally; what is still untested and why).
   - Screenshots / curl examples for UI / API changes.
6. Address review feedback by pushing additional commits (we squash on merge — keep history readable in the meantime).

## Getting help

- Architecture / design questions: open a GitHub Discussion or comment on the relevant `openspec/changes/` doc.
- Bugs that are not security-sensitive: regular GitHub issues.
- Security: see [SECURITY.md](SECURITY.md).
- Conduct concerns: see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

There is no Discord/Slack yet — we'll set one up once the contributor base is large enough that GitHub asynchronous channels stop being enough.
