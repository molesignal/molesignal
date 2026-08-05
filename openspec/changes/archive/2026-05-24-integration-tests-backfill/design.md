## Context

15 套集成测试缺口分布在多个已归档 change（production-core-engine、feature-parity 等）。原因有二：(1) docker 测试需要 testcontainer，本地 CI 之前没拉起；(2) M1 优先 unit test 把核心算法盖住，端到端留到能跑 docker 的版本再补。

现在 docker 环境已就绪、existing `tests/common::TestServer` fixture 已稳定，是时候补完。

## Goals / Non-Goals

**Goals:**
- 15 套 it 测试全部 land
- 每条覆盖 happy + sad 两路径（最少）
- 全跑约 3 分钟在 CI 里能并行 fail-fast
- 不破坏现有 18 套已 land 测试

**Non-Goals:**
- 不重构 `tests/common` fixture（除非新测试缺东西，那时增量 helper）
- 不做 fuzz / property-based 测试（独立 PR）
- 不做 load / soak 测试（属 staging 范畴）

## Decisions

### D1：每条测试一个 `it_*.rs` 文件

跟现有约定一致。一个文件 = 一个 cargo test target，并行单位明确，失败 stack trace 隔离。

### D2：fixture `TestServer::start()` 是 per-test

每条 it 跑前新起一个 postgres + axum + 自己 truncate；3 个 `#[tokio::test]` 在一个文件里就是 3 个独立的容器。容器启动 + migration ~3s，可接受。

### D3：`wiremock` 用于外部 webhook / HTTP 服务 mock

scheduled_reports / connectors / domain ACME 都需要模拟外部 HTTP server。`wiremock` 是 Rust 标准，与 `reqwest` 同 stack。

### D4：sad path 要明确 status code + 错误体

不只是"不 200 就行"，应当断言具体的 status code 和 error message 关键词，便于 regression 时定位。

### D5：cargo test --test-threads=1

每条 test 自起 pg container 占资源；并行 > 4 容易触 Mac docker desktop memory limit。`--test-threads=1` 在 CI 单线程跑（per-binary 内部 `#[tokio::test]` 还可以多线程，因为 binary 内共享 process）。

### D6：跨 it 不共享 schema

每个 binary 拿一个全新 pg → migration 跑一次 → 该 binary 内 3 个 test 共享。这样 binary 启动开销摊给 3 个 test。

## Risks / Trade-offs

**[R1] CI 总耗时增加 ~3min**
→ Mitigation：单独 job 跑，与 lib test 并行；matrix 分 OSS + enterprise 两 job。

**[R2] wiremock 端口冲突**
→ Mitigation：wiremock 自动选随机端口；不写死。

**[R3] flaky 因 timing**
→ Mitigation：用 `wait_until(deadline, predicate)` helper 而不是 `tokio::time::sleep(N)`；最多等 5s，否则 fail.

**[R4] 测试维护成本**
→ Mitigation：每条 < 100 行；helpers 提到 common；spec 行为变了 → grep 测试名 + 同 PR 改测试。
