################ Stage 1: build rust binary ################
FROM rust:1.96-bookworm AS rust-builder
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
FROM debian:bookworm-slim
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
