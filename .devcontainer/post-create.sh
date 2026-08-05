#!/usr/bin/env bash
# 容器首次创建后执行（VS Code devcontainer postCreateCommand）。
# 任务：预热依赖缓存 + 安装 npm/cargo binary tools 二次确认 + 跑一次 sanity check。
set -euo pipefail

cd /workspace

echo "==> rustc / cargo / node / pnpm 版本"
rustc --version
cargo --version
node --version
pnpm --version
buf --version 2>/dev/null || echo "(buf optional)"
openspec --version 2>/dev/null || echo "(openspec optional)"

echo
echo "==> cargo fetch（预热依赖）"
cargo fetch || true

echo
echo "==> pnpm install（web 前端）"
if [ -d web ]; then
  ( cd web && pnpm install --frozen-lockfile ) || ( cd web && pnpm install )
else
  echo "  (no web/ dir, skip)"
fi

echo
echo "==> sqlx migrate run（如有 DATABASE_URL 且 sqlx-cli 在）"
if command -v sqlx >/dev/null 2>&1; then
  export DATABASE_URL="${DATABASE_URL:-postgres://molesignal:molesignal@postgres:5432/molesignal}"
  ( cd crates/infra && sqlx migrate run --source migrations ) || \
    echo "  (sqlx migrate skipped or failed; DB may not be ready yet)"
else
  echo "  (sqlx-cli not installed, skip)"
fi

cat <<'EOF'

==============================================================
 molesignal dev container ready.

  Postgres:   postgres:5432  (molesignal/molesignal/molesignal)
  MinIO S3:   http://minio:9000  (minioadmin/minioadmin)
  MinIO Web:  http://localhost:9001  (port-forwarded)

 常用命令：
   cargo run -p molesignal-bootstrap -- --config conf/config.toml
   cargo test --workspace --lib
   ( cd web && pnpm dev )                # vite on :5173

 集成测试（已编译；需 docker-out-of-docker）：
   MS_RUN_IT=1 cargo test --workspace --test 'it_*'

==============================================================
EOF
