#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 分配器 RSS / 吞吐对照：system(glibc) vs glibc-tuned vs jemalloc。
#
# 用 examples/alloc_bench（Arrow+DataFusion+parquet 高 churn 负载）各跑一遍，对比
# 吞吐(queries/s)、峰值 RSS、负载停止+idle 后 RSS（看分配器是否把内存还给 OS）。
#
# 用法：
#   scripts/alloc_bench.sh            # 本机跑（Linux glibc 才有代表性；macOS 仅吞吐有意义）
#   scripts/alloc_bench.sh --docker   # 在项目 glibc 镜像(debian bookworm)内跑，贴合生产【推荐】
#
# 可调（env）：BENCH_SECONDS=20 BENCH_IDLE_SECONDS=8 BENCH_ROWS=20000 BENCH_BATCHES=12 BENCH_THREADS=<n>
set -euo pipefail

: "${BENCH_SECONDS:=20}"
: "${BENCH_IDLE_SECONDS:=8}"
: "${BENCH_ROWS:=20000}"
: "${BENCH_BATCHES:=12}"
: "${BENCH_THREADS:=}"

# ---- --docker：在 rust:1.96-bookworm（与 Dockerfile builder 同）内跑，拿真实 glibc 数字 ----
if [ "${1:-}" = "--docker" ]; then
  command -v docker >/dev/null || { echo "docker 不可用"; exit 1; }
  echo ">>> 在 rust:1.96-bookworm 容器内跑（首跑要编 arrow/datafusion/jemalloc，约 10-15 min；之后走缓存卷）"
  exec docker run --rm -i \
    -v "$PWD":/src -w /src \
    -v molesignal_alloc_cargo:/usr/local/cargo/registry \
    -v molesignal_alloc_target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e BENCH_SECONDS -e BENCH_IDLE_SECONDS -e BENCH_ROWS -e BENCH_BATCHES -e BENCH_THREADS \
    -e CARGO_BUILD_JOBS \
    rust:1.96-bookworm bash -c '
      set -e
      if ! command -v buf >/dev/null; then
        apt-get update -qq && apt-get install -y -qq protobuf-compiler curl ca-certificates >/dev/null
        curl -fsSL "https://github.com/bufbuild/buf/releases/latest/download/buf-Linux-$(uname -m)" -o /usr/local/bin/buf
        chmod +x /usr/local/bin/buf
      fi
      exec bash scripts/alloc_bench.sh
    '
fi

OS="$(uname -s)"
TGT="${CARGO_TARGET_DIR:-target}"
if [ "$OS" != "Linux" ]; then
  echo "!! 警告：当前不是 Linux (${OS})。生产是 glibc Linux —— macOS 上 MALLOC_ARENA_MAX 无效、"
  echo "!! 且 RSS 自报不可用（显示 n/a），只有吞吐有参考意义。要可信的 RSS 对照请加 --docker。"
  echo
fi

# 放宽 release 的 lto/codegen-units：项目默认 lto=thin + codegen-units=1 会让单个 rustc
# 内存峰值很高，在内存受限的容器里编 datafusion/sqlparser 会被 OOM-kill。分配器对照是
# 相对值（三档用同一构建），绝对吞吐略低不影响结论。可用 CARGO_BUILD_JOBS 进一步限并发。
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
: "${CARGO_BUILD_JOBS:=4}"; export CARGO_BUILD_JOBS

echo ">>> 构建两个变体（release, lto=off cu=16 -j${CARGO_BUILD_JOBS}）..."
cargo build --release -q -p molesignal-infra --example alloc_bench
cp "$TGT/release/examples/alloc_bench" /tmp/alloc_bench_system
cargo build --release -q -p molesignal-infra --example alloc_bench --features jemalloc
cp "$TGT/release/examples/alloc_bench" /tmp/alloc_bench_jemalloc

# 传给 bench 的 env（BENCH_THREADS 为空则不传，让 bench 用 CPU 数）
common_env=(BENCH_SECONDS="$BENCH_SECONDS" BENCH_IDLE_SECONDS="$BENCH_IDLE_SECONDS" \
            BENCH_ROWS="$BENCH_ROWS" BENCH_BATCHES="$BENCH_BATCHES")
[ -n "$BENCH_THREADS" ] && common_env+=(BENCH_THREADS="$BENCH_THREADS")

echo ">>> workload: ${BENCH_SECONDS}s 负载 + ${BENCH_IDLE_SECONDS}s idle，rows=$BENCH_ROWS batches=$BENCH_BATCHES threads=${BENCH_THREADS:-auto}"
echo

run_cfg() { # $1=label  $2=binary  $3.. = 额外 env
  local label="$1" bin="$2"; shift 2
  echo "=== $label ===" >&2
  env "${common_env[@]}" "$@" "$bin"
}

OUT_SYS="$(run_cfg '① system (glibc 默认)' /tmp/alloc_bench_system)"
OUT_TUNED="$(run_cfg '② glibc 调参 (ARENA_MAX=2 + trim)' /tmp/alloc_bench_system \
            MALLOC_ARENA_MAX=2 MALLOC_TRIM_THRESHOLD_=131072)"
OUT_JE="$(run_cfg '③ jemalloc (background_thread)' /tmp/alloc_bench_jemalloc \
         _RJEM_MALLOC_CONF=background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000 \
         MALLOC_CONF=background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000)"

val() { echo "$1" | grep -oE "$2=[-0-9.]+" | cut -d= -f2; }

printf '\n%s\n' "================== 结果对照 =================="
printf '%-26s %14s %14s %16s\n' "配置" "吞吐(q/s)" "峰值RSS(MB)" "idle后RSS(MB)"
row() { # $1 label  $2 OUT
  local q peak post
  q="$(val "$2" THROUGHPUT_QPS)"
  peak="$(val "$2" RSS_PEAK_KB)"; post="$(val "$2" RSS_POSTIDLE_KB)"
  awk -v l="$1" -v q="$q" -v pk="$peak" -v po="$post" 'BEGIN{
    pm = (pk<0)?"n/a":sprintf("%.0f", pk/1024);
    qm = (po<0)?"n/a":sprintf("%.0f", po/1024);
    printf "%-24s %14.1f %14s %16s\n", l, q, pm, qm;
  }'
}
row "① system(glibc)"      "$OUT_SYS"
row "② glibc 调参"          "$OUT_TUNED"
row "③ jemalloc"            "$OUT_JE"

# 相对 ① 的增量（仅 Linux 有 RSS）
awk -v s="$(val "$OUT_SYS" THROUGHPUT_QPS)" -v j="$(val "$OUT_JE" THROUGHPUT_QPS)" \
    -v sp="$(val "$OUT_SYS" RSS_POSTIDLE_KB)" -v jp="$(val "$OUT_JE" RSS_POSTIDLE_KB)" 'BEGIN{
  printf "\n相对 ①：jemalloc 吞吐 %+.1f%%", (j-s)/s*100;
  if (sp>0 && jp>0) printf " · idle后RSS %+.1f%%（负=更省）", (jp-sp)/sp*100;
  printf "\n";
}'

cat <<'EOF'

读法：
  - 吞吐越高越好；峰值 RSS / idle 后 RSS 越低越好。
  - "idle 后 RSS"最关键：它看负载退去后内存有没有还给 OS——glibc 常"plateau"在峰值不降，
    jemalloc(background_thread)/glibc 调参通常显著降低，这正是容器内 OOM 风险 / 部署密度的来源。
  - macOS 本机的 RSS 显示 n/a 且 ② 无效；要给生产决策用，请 `--docker` 或在 Linux 上跑。
EOF
