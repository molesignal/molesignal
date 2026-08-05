# molesignal devcontainer

VS Code / Cursor / 任何 [Dev Containers spec](https://containers.dev) 兼容编辑器都能用。

## 包含

| Layer | What |
|---|---|
| Base image | `mcr.microsoft.com/devcontainers/rust:1-1-bookworm` |
| Rust | 1.90（与 `rust-toolchain.toml` 对齐）+ rustfmt + clippy + rust-src |
| Node | 20 + pnpm 9（`web/` 前端用） |
| Proto | `protoc` + `buf` 1.50 |
| DB tools | `psql` + `sqlx-cli`（postgres feature） |
| Dev tools | `cargo-watch` + `openspec` CLI + `gh` + `jq` |
| Services | Postgres 17 + MinIO（来自 `deploy/docker/docker-compose.yaml`） |

## 启动

**VS Code / Cursor:** 打开仓库根目录 → 命令面板 `Dev Containers: Reopen in Container` → 等首次构建（~5-8 分钟，含 `cargo fetch`）。

**命令行（不用 IDE 也行）:**
```bash
docker compose -f .devcontainer/docker-compose.yml up -d
docker compose -f .devcontainer/docker-compose.yml exec workspace bash
```

## 容器内常用命令

```bash
# 主服务（standalone 模式，HTTP 5080 / gRPC 5082）
cargo run -p molesignal-bootstrap -- --config conf/config.toml

# 单测
cargo test --workspace --lib
cd  && cargo test --workspace

# 启付费版构建
cargo build -p molesignal-bootstrap --features 

# 前端 dev server（vite，端口 5173 已转发）
cd web && pnpm dev

# 集成测试（需要 docker-out-of-docker，已启用）
MS_RUN_IT=1 cargo test --workspace --test 'it_*'

# openspec 工作流
openspec list
openspec validate <change-name> --strict
```

## 转发端口

| Port | 用途 |
|---|---|
| 5080 | molesignal HTTP API |
| 5082 | molesignal gRPC（OTLP / Arrow Flight） |
| 5173 | Vite dev server（web/） |
| 5432 | Postgres |
| 9000 | MinIO S3 API |
| 9001 | MinIO Web Console |

## Volume

为提速避免每次 rebuild 重下：
- `molesignal-cargo-cache` → `/usr/local/cargo/registry`
- `molesignal-target` → `/workspace/target`
- `molesignal-pnpm-cache` → `/home/vscode/.local/share/pnpm/store`

清空方法（在 host 跑）：
```bash
docker volume rm molesignal-cargo-cache molesignal-target molesignal-pnpm-cache
```

## 注入的环境变量

容器启动就有，可直接 `cargo run` 不写 `--config`：

| Var | Value |
|---|---|
| `MS_STORE_META_DSN` | `postgres://molesignal:molesignal@postgres:5432/molesignal` |
| `MS_STORE_OBJECT_*` | 指向 compose 内的 minio |
| `MS_CIPHER_KEY` | 32 字节全零 base64（**仅 dev**，cipher_keys envelope KEK；auth-hardening） |
| `RUST_LOG` | `molesignal=debug,info` |

## 已预装 VS Code 扩展

rust-analyzer / dependi / Even Better TOML / CodeLLDB / buf / ESLint / Prettier / Volar / Tailwind / GitLens / Copilot / Markdown All-in-One。

## Troubleshooting

**容器首启很慢？** 第一次构建 image + `cargo fetch` 全部依赖确实要几分钟。之后 named volume 缓存住，重开秒级。

**MinIO bucket 不存在？** `minio-init` 服务每次启动会幂等地 `mc mb local/molesignal`，等它 `condition: service_completed_successfully` 就好。

**sqlx-cli migrate 报"DB not ready"？** `depends_on: condition: service_healthy` 已等 Postgres healthcheck 过；若仍失败重跑 `bash .devcontainer/post-create.sh`。

**想用本地 cargo 缓存（不用 volume）？** 删除 `devcontainer.json` 里的 `mounts` 段，target/ 会落到 bind-mount 的代码目录，但跨 host/container 文件系统会慢 5-10x。
