################ Stage 1: build rust binary ################
# bullseye（Debian 11, glibc 2.31）：产物 glibc 地板 2.31，覆盖目标机地板 2.34
# （Alma 9）与 Ubuntu 22.04（2.35）。bookworm/trixie 的 glibc 更高，会在这类
# 目标机上报 "GLIBC_2.x not found"。
FROM rust:1.96-bullseye AS rust-builder
ARG BUILD_ID=unknown
ARG GIT_SHA=unknown
# protobuf binding 已提交到 src/protocol；make proto 仍可手动刷新。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl protobuf-compiler \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL "https://github.com/bufbuild/buf/releases/latest/download/buf-Linux-$(uname -m)" \
      -o /usr/local/bin/buf \
 && chmod +x /usr/local/bin/buf \
 && buf --version

WORKDIR /src
COPY Cargo.toml Cargo.lock* rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY build.rs ./
COPY proto ./proto
COPY src ./src
RUN BUILD_ID="$BUILD_ID" GIT_SHA="$GIT_SHA" cargo build --release --locked --bin molesignal

################ Stage 2: runtime ################
# bullseye-slim（glibc 2.31）：镜像自带 glibc 低于目标机地板 2.34（Alma 9 /
# Ubuntu 22.04），且 ≥ 二进制地板 2.31，二者自洽。
# ⚠ bullseye 的 chromium 已 EOL（120.x，2024 起无安全更新）。OSS build 不调
# renderer，纯 OSS 部署不受影响；如用 scheduled-reports 的 PDF/PNG 渲染，建议
# 删除 chromium 并考虑独立渲染镜像，或接受 EOL 风险。
FROM debian:bullseye-slim
#  build 用 headless-Chrome 渲染 scheduled-reports 的 PDF/PNG
# （change `scheduled-reports-headless`）。OSS build 不会调 renderer，但额外几个
# 包 + chromium 多了 ~150MB；如要纯 OSS 最小镜像，删除 `chromium` 即可。
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates curl \
        chromium fonts-liberation libnss3 libfreetype6 libharfbuzz0b \
 && rm -rf /var/lib/apt/lists/*
# headless_chrome 默认会扫这些路径找 Chromium 可执行文件。
ENV CHROME_PATH=/usr/bin/chromium
WORKDIR /app
COPY --from=rust-builder /src/target/release/molesignal /usr/local/bin/molesignal
RUN mkdir -p /usr/share/molesignal/mmdb
COPY assets/mmdb/GeoLite2-City.mmdb /usr/share/molesignal/mmdb/GeoLite2-City.mmdb
COPY conf/config.toml /app/conf/config.toml
EXPOSE 5080 5082
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -fsS http://localhost:5080/api/v1/healthz || exit 1
ENTRYPOINT ["molesignal"]
CMD ["--config", "/app/conf/config.toml"]
