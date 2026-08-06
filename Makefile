# molesignal Makefile
# ============================================================================
# Rust workspace (单二进制 molesignal) + Web 前端 (pnpm/Vite) + buf proto +
# docker/k8s 部署。
# ============================================================================

BIN_NAME     := molesignal
BIN_PKG      := molesignal
CONFIG_DIR   := conf
DATA_DIR     := data
ASSET_DIR    := assets
MMDB_DIR     := $(ASSET_DIR)/mmdb
MMDB_FILE    := $(MMDB_DIR)/GeoLite2-City.mmdb
DIST_DIR     := dist
PACKAGE_NAME = $(BIN_NAME)-$(VERSION)
PACKAGE_DIR  = $(DIST_DIR)/$(PACKAGE_NAME)
PACKAGE_BIN_DIR = $(if $(TARGET),target/$(TARGET)/release,target/release)
HTTP_PORT    := 5080
WEB_DIR      := web
DEPLOY_DIR   := deploy
COMPOSE_FILE := $(DEPLOY_DIR)/docker/docker-compose.yaml
DOCKERFILE   := $(DEPLOY_DIR)/docker/Dockerfile
DOCKERFILE_WEB := $(DEPLOY_DIR)/docker/Dockerfile.web

# --- 版本管理 ---
# 优先级: 环境变量 VERSION → VERSION 文件 → Git 标签 → Cargo.toml workspace.package.version → 默认值
# 采用 SemVer (Major.Minor.Patch[-prerelease])。
VERSION_FILE := VERSION

ifndef VERSION
  ifneq (,$(wildcard $(VERSION_FILE)))
    VERSION := $(shell cat $(VERSION_FILE) | tr -d '[:space:]')
  else
    GIT_TAG_VERSION := $(shell git describe --tags --exact-match HEAD 2>/dev/null | sed 's/^v//')
    ifneq (,$(GIT_TAG_VERSION))
      VERSION := $(GIT_TAG_VERSION)
    else
      CARGO_VERSION := $(shell sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n1)
      ifneq (,$(CARGO_VERSION))
        VERSION := $(CARGO_VERSION)
      else
        VERSION := 0.1.0
      endif
    endif
  endif
endif
export VERSION

# Git 元信息（用于 -X 注入或镜像标签）
GIT_COMMIT   := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)
GIT_SHA      := $(shell git rev-parse HEAD 2>/dev/null || echo unknown)
BUILD_DATE   := $(shell date -u +%Y-%m-%dT%H:%M:%SZ)
BUILD_ID     ?= local-$(GIT_COMMIT)
RELEASE_CHANNEL ?= alpha

# 构建目标
RUST_TARGETS := \
    x86_64-unknown-linux-musl \
    aarch64-unknown-linux-musl \
    aarch64-apple-darwin

DOCKER_PLATFORMS := linux/amd64,linux/arm64
DOCKER_IMAGE     ?= molesignal
DOCKER_IMAGE_WEB ?= molesignal-web
DOCKER_TAG       ?= $(VERSION)

# Cargo 通用参数
CARGO_FLAGS_BASE := --frozen --locked
WORKSPACE_PKGS   := -p $(BIN_PKG)

# 付费版 feature（默认关）。使用：make build FEATURES=
ifdef FEATURES
  CARGO_FEATURE_FLAGS := --features $(FEATURES)
else
  CARGO_FEATURE_FLAGS :=
endif

.PHONY: all
all: build

# === 构建 ===
.PHONY: build build-release build-debug
build:
	$(MAKE) build-release

build-debug:
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

build-release:
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) --release $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

# 生成二进制发布包：内置 conf/ 和 systemd 部署文件
package:
	@if [ -n "$(TARGET)" ]; then \
		$(MAKE) build-release-target TARGET=$(TARGET); \
	else \
		$(MAKE) build-release; \
	fi
	rm -rf "$(PACKAGE_DIR)"
	mkdir -p "$(PACKAGE_DIR)/bin" "$(PACKAGE_DIR)/conf" "$(PACKAGE_DIR)/deploy/systemd"
	cp "$(PACKAGE_BIN_DIR)/$(BIN_NAME)" "$(PACKAGE_DIR)/bin/$(BIN_NAME)"
	cp -R "$(CONFIG_DIR)/." "$(PACKAGE_DIR)/conf/"
	cp -R "$(DEPLOY_DIR)/systemd/." "$(PACKAGE_DIR)/deploy/systemd/"
	printf '{"build_id":"%s","git_sha":"%s"}\n' "$(BUILD_ID)" "$(GIT_SHA)" > "$(PACKAGE_DIR)/build-info.json"
	tar -C "$(DIST_DIR)" -czf "$(DIST_DIR)/$(PACKAGE_NAME).tar.gz" "$(PACKAGE_NAME)"
	@echo "→ wrote $(DIST_DIR)/$(PACKAGE_NAME).tar.gz"

# 为指定 target 构建，用法: make build-release-target TARGET=x86_64-unknown-linux-musl
.PHONY: build-release-target
build-release-target:
	@if [ -z "$(TARGET)" ]; then \
		echo "Usage: make build-release-target TARGET=<rust-target>"; \
		echo "  e.g. make build-release-target TARGET=x86_64-unknown-linux-musl"; \
		echo "  Available: $(RUST_TARGETS)"; \
		exit 1; \
	fi
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) --release --target $(TARGET) $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

# 跨平台快捷目标（Linux 需在对应架构机器并安装 musl-tools）
.PHONY: build-linux-amd64 build-linux-arm64 build-darwin-arm64
build-linux-amd64:
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) --release --target x86_64-unknown-linux-musl $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

build-linux-arm64:
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) --release --target aarch64-unknown-linux-musl $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

build-darwin-arm64:
	BUILD_ID="$(BUILD_ID)" cargo build $(CARGO_FLAGS_BASE) --release --target aarch64-apple-darwin $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

OUTPUT_DIR = target/$(TARGET)/release

# === 运行 ===
.PHONY: run run-debug run-release
run: run-debug

run-debug:
	RELEASE_CHANNEL="$(RELEASE_CHANNEL)" BUILD_ID="$(BUILD_ID)" cargo run $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS) -- --config $(CONFIG_DIR)/config.toml

run-release:
	RELEASE_CHANNEL="$(RELEASE_CHANNEL)" BUILD_ID="$(BUILD_ID)" cargo run --release $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS) -- --config $(CONFIG_DIR)/config.toml

# === 测试 ===
.PHONY: test test-unit test-integration test-all
test: test-unit

test-unit:
	cargo test $(CARGO_FLAGS_BASE) --workspace --lib --bins $(CARGO_FEATURE_FLAGS)

test-integration:
	cargo test $(CARGO_FLAGS_BASE) --workspace --tests $(CARGO_FEATURE_FLAGS)

test-all:
	cargo test $(CARGO_FLAGS_BASE) --workspace --all-targets $(CARGO_FEATURE_FLAGS)

# === Bench ===
.PHONY: bench
bench:
	cargo bench $(CARGO_FLAGS_BASE) --workspace $(CARGO_FEATURE_FLAGS)

# === Git hooks ===
.PHONY: install-hooks
install-hooks:
	git config core.hooksPath .githooks
	chmod +x .githooks/*
	@echo "==> git hooks 已启用 (core.hooksPath = .githooks)"

# === 代码质量 ===
.PHONY: fmt fmt-check lint lint-fix check check-all add-license-headers check-license-headers _ensure-nightly-rustfmt
add-license-headers:
	bash scripts/license-headers.sh

check-license-headers:
	bash scripts/license-headers.sh --check

# rustfmt.toml 含 nightly-only 选项 (imports_granularity / group_imports)；
# fmt 必须用 nightly rustfmt 才能让规则生效（与 CI 一致）。
_ensure-nightly-rustfmt:
	@rustup run nightly rustfmt --version >/dev/null 2>&1 || { \
	    echo "ERROR: 未检测到 nightly rustfmt。请运行："; \
	    echo "  rustup toolchain install nightly --profile minimal --component rustfmt"; \
	    exit 1; \
	}

fmt: _ensure-nightly-rustfmt
	cargo +nightly fmt --all

fmt-check: _ensure-nightly-rustfmt
	cargo +nightly fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets $(CARGO_FEATURE_FLAGS) -- -D warnings

lint-fix:
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged $(CARGO_FEATURE_FLAGS) -- -D warnings

check:
	cargo check $(WORKSPACE_PKGS) $(CARGO_FEATURE_FLAGS)

check-all:
	cargo check --workspace --all-targets $(CARGO_FEATURE_FLAGS)

# === Proto 代码生成 ===
# 注：proto 代码生成现为手动执行（build.rs 不再自动触发 buf），输出到 src/protocol/。
.PHONY: proto proto-lint proto-breaking
proto:
	cd proto && buf generate

proto-lint:
	buf lint

proto-breaking:
	buf breaking --against '.git#branch=main'

# === 统一入口（后端 + 前端）===
.PHONY: ci ci-fast
# 提交前自检：格式、lint、测试、web 检查
ci: fmt-check lint test web-typecheck web-lint web-test

# 不跑测试的快速门禁
ci-fast: fmt-check lint check-all web-typecheck web-lint

# === Docker ===
.PHONY: docker-build docker-build-web docker-build-multi docker-push docker-run
docker-build:
	docker build --build-arg BUILD_ID="$(BUILD_ID)" --build-arg GIT_SHA="$(GIT_SHA)" -f $(DOCKERFILE) -t $(DOCKER_IMAGE):$(DOCKER_TAG) -t $(DOCKER_IMAGE):latest .

docker-build-web:
	docker build -f $(DOCKERFILE_WEB) -t $(DOCKER_IMAGE_WEB):$(DOCKER_TAG) -t $(DOCKER_IMAGE_WEB):latest .

# 多平台镜像（需 buildx）
docker-build-multi:
	docker buildx build --build-arg BUILD_ID="$(BUILD_ID)" --build-arg GIT_SHA="$(GIT_SHA)" --platform $(DOCKER_PLATFORMS) -f $(DOCKERFILE) -t $(DOCKER_IMAGE):$(DOCKER_TAG) --push .

docker-push:
	docker push $(DOCKER_IMAGE):$(DOCKER_TAG)
	docker push $(DOCKER_IMAGE):latest

docker-run:
	docker run --rm -p $(HTTP_PORT):$(HTTP_PORT) \
		--env RELEASE_CHANNEL="$(RELEASE_CHANNEL)" \
		-v $(PWD)/$(DATA_DIR):/app/data \
		-v $(PWD)/$(CONFIG_DIR):/app/conf \
		$(DOCKER_IMAGE):$(DOCKER_TAG) /app/$(BIN_NAME) --config /app/conf/config.toml

# === 版本管理 ===
.PHONY: version version-check version-tag version-set version-bump-patch version-bump-minor version-bump-major

# 显示当前版本
version:
	@echo "$(VERSION)"

# 校验 SemVer 格式 (Major.Minor.Patch[-prerelease])
version-check:
	@echo "$(VERSION)" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$$' \
		&& echo "✓ version $(VERSION) is valid (SemVer)" \
		|| (echo "✗ version $(VERSION) is INVALID (expected Major.Minor.Patch[-prerelease])" && exit 1)

# 写入 VERSION 文件：make version-set VERSION=0.2.0
version-set:
	@if [ -z "$(VERSION)" ]; then echo "VERSION is not set"; exit 1; fi
	@$(MAKE) version-check VERSION=$(VERSION)
	@echo "$(VERSION)" > $(VERSION_FILE)
	@echo "→ wrote $(VERSION) to $(VERSION_FILE)"
	@echo "  remember: also bump workspace.package.version in Cargo.toml"

# 同步 VERSION 到 Cargo.toml workspace.package.version
.PHONY: version-sync-cargo
version-sync-cargo:
	@$(MAKE) version-check
	@if [ "$$(uname)" = "Darwin" ]; then \
		sed -i '' -E 's/^(version[[:space:]]*=[[:space:]]*)"[^"]+"/\1"$(VERSION)"/' Cargo.toml; \
	else \
		sed -i -E 's/^(version[[:space:]]*=[[:space:]]*)"[^"]+"/\1"$(VERSION)"/' Cargo.toml; \
	fi
	@echo "→ synced Cargo.toml workspace.package.version to $(VERSION)"

# 创建 Git tag (vX.Y.Z)
version-tag:
	@if [ -z "$(VERSION)" ]; then echo "VERSION is not set"; exit 1; fi
	@$(MAKE) version-check
	@if git rev-parse "v$(VERSION)" >/dev/null 2>&1; then \
		echo "✗ tag v$(VERSION) already exists"; exit 1; \
	fi
	git tag -a "v$(VERSION)" -m "Release version $(VERSION)"
	@echo "→ created tag v$(VERSION) (push with: git push origin v$(VERSION))"

# Bump 版本号（基于当前 VERSION，剥掉 -prerelease 后再 bump）
_BASE_VERSION := $(shell echo $(VERSION) | sed 's/-.*//')
_V_MAJOR := $(word 1,$(subst ., ,$(_BASE_VERSION)))
_V_MINOR := $(word 2,$(subst ., ,$(_BASE_VERSION)))
_V_PATCH := $(word 3,$(subst ., ,$(_BASE_VERSION)))

version-bump-patch:
	@new=$$(printf "%d.%d.%d" $(_V_MAJOR) $(_V_MINOR) $$(( $(_V_PATCH) + 1 ))); \
	echo "$$new" > $(VERSION_FILE); \
	echo "→ bumped patch: $(VERSION) → $$new"

version-bump-minor:
	@new=$$(printf "%d.%d.0" $(_V_MAJOR) $$(( $(_V_MINOR) + 1 ))); \
	echo "$$new" > $(VERSION_FILE); \
	echo "→ bumped minor: $(VERSION) → $$new"

version-bump-major:
	@new=$$(printf "%d.0.0" $$(( $(_V_MAJOR) + 1 ))); \
	echo "$$new" > $(VERSION_FILE); \
	echo "→ bumped major: $(VERSION) → $$new"

# 发布前完整流程：bump → 同步 Cargo.toml → 检查 → 提示
.PHONY: release-prepare
release-prepare:
	@$(MAKE) version-check
	@$(MAKE) version-sync-cargo
	@echo ""
	@echo "next steps:"
	@echo "  1) review changes:  git diff Cargo.toml $(VERSION_FILE)"
	@echo "  2) commit:          git commit -am 'chore: release v$(VERSION)'"
	@echo "  3) tag:             make version-tag"
	@echo "  4) push:            git push && git push origin v$(VERSION)"

# === 工具链 / 初始化 ===
.PHONY: setup install-targets
setup:
	@if [ -d .githooks ]; then git config core.hooksPath .githooks; fi
	rustup show
	@command -v buf >/dev/null 2>&1 || echo "warn: buf not installed (https://buf.build/docs/installation)"
	@command -v pnpm >/dev/null 2>&1 || echo "warn: pnpm not installed (npm i -g pnpm)"

install-targets:
	@for t in $(RUST_TARGETS); do rustup target add $$t; done

# === 清理 ===
.PHONY: clean clean-all
clean:
	cargo clean

clean-all: clean
	rm -rf target /target

# === 帮助 ===
.PHONY: help
help:
	@echo "molesignal Makefile  (version: $(VERSION), commit: $(GIT_COMMIT))"
	@echo ""
	@echo "构建:"
	@echo "  make build / build-release      - 统一 release 构建"
	@echo "  make build-debug                - debug 构建"
	@echo "  make package                    - 构建二进制发布包"
	@echo "  make build-release-target TARGET=<rust-target>"
	@echo "  make build-linux-amd64 / build-linux-arm64 / build-darwin-arm64"
	@echo "  BUILD_ID= 可显式指定不可变构建标识"
	@echo "  FEATURES= 可叠加付费版 feature"
	@echo ""
	@echo "运行:"
	@echo "  make run / run-release          - 本地启动（RELEASE_CHANNEL 默认 alpha）"
	@echo ""
	@echo "测试 / 质量:"
	@echo "  make test / test-integration / test-all"
	@echo "  make bench"
	@echo "  make fmt / fmt-check / lint / lint-fix / check / check-all"
	@echo "  make ci / ci-fast               - 组合门禁"
	@echo ""
	@echo "Proto:"
	@echo "  make proto / proto-lint / proto-breaking"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build / docker-build-web"
	@echo "  make docker-build-multi (buildx)"
	@echo "  make docker-run"
	@echo ""
	@echo "版本:"
	@echo "  make version                    - 显示当前版本"
	@echo "  make version-check              - 校验 SemVer 格式"
	@echo "  make version-set VERSION=X.Y.Z  - 写入 VERSION 文件"
	@echo "  make version-sync-cargo         - 同步到 Cargo.toml"
	@echo "  make version-bump-patch / minor / major"
	@echo "  make version-tag                - 创建 git tag v\$$VERSION"
	@echo "  make release-prepare            - 发布前流程"
	@echo ""
	@echo "其他:"
	@echo "  make setup                      - 安装 git hook 并检测工具链"
	@echo "  make install-targets            - 安装跨平台 Rust target"
	@echo "  make clean / clean-all"
