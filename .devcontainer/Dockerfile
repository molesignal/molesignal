# molesignal dev container —— Rust 1.90 + Node 20 + buf + openspec + sqlx-cli + 常用工具
#
# Base：Microsoft devcontainers Rust 镜像（已含 vscode 用户、git、build-essential、lldb 等）
FROM mcr.microsoft.com/devcontainers/rust:1-1-bookworm

# 锁 Rust 工具链版本（与 rust-toolchain.toml 对齐）
ARG RUST_TOOLCHAIN=1.90
RUN rustup install ${RUST_TOOLCHAIN} \
 && rustup default ${RUST_TOOLCHAIN} \
 && rustup component add rustfmt clippy rust-src

# 系统依赖：Postgres client（psql 调试用）、protobuf、curl、unzip、ca-certs、libssl
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        postgresql-client \
        protobuf-compiler \
        libssl-dev \
        pkg-config \
        curl unzip ca-certificates jq \
        cmake \
        clang \
        lld \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Node.js 20 + pnpm 9（web/ 前端用）
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && apt-get clean && rm -rf /var/lib/apt/lists/* \
 && npm install -g pnpm@9 \
 && pnpm config set store-dir /home/vscode/.local/share/pnpm/store

# buf —— proto lint + breaking-change check
ARG BUF_VERSION=1.50.0
RUN curl -fsSL "https://github.com/bufbuild/buf/releases/download/v${BUF_VERSION}/buf-Linux-x86_64" \
        -o /usr/local/bin/buf \
 && chmod +x /usr/local/bin/buf

# openspec CLI —— spec workflow（用户走 /opsx:apply / /opsx:archive 用）
RUN npm install -g @fungiboletus/openspec || \
    npm install -g openspec || true

# sqlx-cli —— 数据库 migration 工具（molesignal 用 sqlx）
RUN cargo install sqlx-cli --no-default-features --features postgres,rustls --locked || true

# cargo-watch —— `cargo watch -x run` 开发用
RUN cargo install cargo-watch --locked || true

# 切回 vscode 用户（base 镜像默认就是它，明示一下）
USER vscode

# 让 pnpm store 落到挂载的 volume（与 devcontainer.json mounts 对齐）
RUN mkdir -p /home/vscode/.local/share/pnpm/store

# 提前把 cargo / rust-analyzer 缓存目录建好（避免 mount 时权限错乱）
RUN mkdir -p /home/vscode/.cargo /home/vscode/.rustup

WORKDIR /workspace
