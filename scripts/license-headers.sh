#!/usr/bin/env bash
# 维护 Rust 源文件版权头（SPDX 风格，两行）。
#
# 用法：
#   scripts/license-headers.sh             # add 模式：给缺头的自有 .rs 文件补齐
#   scripts/license-headers.sh --check     # check 模式：列出缺头文件，非零退出
#
# 排除范围：
#   - vendored / backport / shim 目录（保留上游版权头）
#   - 含 @generated / DO NOT EDIT 标记的自动生成代码
set -euo pipefail

MODE="${1:-add}"

HEADER_LINE_1="// SPDX-License-Identifier: Apache-2.0"
HEADER_LINE_2="// Copyright (c) 2026 MoleSignal Authors"

EXCLUDE_DIRS=(
    "src/sqlx-shim"
)

cd "$(git rev-parse --show-toplevel)"

missing=()
added=0
skipped=0
total=0

while IFS= read -r f; do
    skip=0
    for d in "${EXCLUDE_DIRS[@]}"; do
        if [[ "$f" == "$d"/* ]]; then
            skip=1
            break
        fi
    done
    [[ $skip -eq 1 ]] && continue

    total=$((total + 1))

    head5="$(head -5 "$f" 2>/dev/null || true)"

    if echo "$head5" | grep -q "SPDX-License-Identifier"; then
        skipped=$((skipped + 1))
        continue
    fi

    if echo "$head5" | grep -qE "@generated|DO NOT EDIT|Automatically generated"; then
        skipped=$((skipped + 1))
        continue
    fi

    if [[ "$MODE" == "--check" ]]; then
        missing+=("$f")
        continue
    fi

    tmp="$(mktemp)"
    printf '%s\n%s\n\n' "$HEADER_LINE_1" "$HEADER_LINE_2" > "$tmp"
    cat "$f" >> "$tmp"
    mv "$tmp" "$f"
    added=$((added + 1))
done < <(find src tests benches examples -name '*.rs' -type f | sort)

if [[ "$MODE" == "--check" ]]; then
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "以下 ${#missing[@]} 个文件缺少版权头："
        printf '  %s\n' "${missing[@]}"
        echo ""
        echo "请运行 'make add-license-headers' 自动补齐。"
        exit 1
    fi
    echo "==> license-header check passed (skipped: $skipped, total: $total)"
else
    echo "==> license-header add: added $added, skipped $skipped, total $total"
fi
