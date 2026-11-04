#!/usr/bin/env python3
"""Claude Code 的 PreToolUse hook：拦下绕过 verify.py 的重型命令。

全量命令只在用户明确要求时才跑，放行靠命令里写 `MS_VERIFY_BYPASS=1`。
脚本出任何异常都放行：拦截只是兜底，不能挡住正常工作。
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import NamedTuple

VERIFY = Path(__file__).resolve().with_name("verify.py")
BYPASS = "MS_VERIFY_BYPASS=1"

HEREDOC = re.compile(r"<<-?\s*(['\"]?)(\w+)\1[^\n]*\n.*?\n\s*\2(?=\s|$)", re.S)
WRAPPER = re.compile(
    r"^(?:\s*(?:[A-Za-z_][A-Za-z0-9_]*=(?:'[^']*'|\"[^\"]*\"|\S*)"
    r"|do|then|else|if|while|until|!|\{"
    r"|nice(?:\s+-n\s*-?\d+)?|time|timeout\s+\S+|gtimeout\s+\S+|caffeinate(?:\s+-\w+)*"
    r"|nohup|exec|env|command|npx(?:\s+--no-install)?|pnpm\s+exec)\s+)+"
)
SHELL_C = re.compile(r"^(?:\S*/)?(?:bash|sh|zsh)(?:\s+-\w+)*\s+-\w*c\s+(['\"])(.*)\1\s*$", re.S)

CARGO = re.compile(r"^(?:\S*/)?cargo(?:\s+\+\S+)?\s+(test|clippy|check|build|bench|nextest)\b")
MAKE = re.compile(r"^(?:\S*/)?make(?:\s+-C\s+\S+)?\s+([\w-]+)")
PLAYWRIGHT = re.compile(
    r"^(?:(?:\S*/)?playwright\s+test\b|(?:pnpm|npm)(?:\s+(?:-C|--dir|--prefix)\s+\S+)?(?:\s+run)?\s+playwright\b"
    r"|\S*playwright/cli\.js\s+test\b)"
)
VITEST = re.compile(r"^(?:\S*/)?vitest(?:\s+run)?(?=\s|$)")
PACKAGE_TEST = re.compile(r"^(?:pnpm|npm)(?:\s+(?:-C|--dir|--prefix)\s+\S+)?(?:\s+run)?\s+(?:test:run|test)(?=\s|$)")
PACKAGE_LINT = re.compile(r"^(?:pnpm|npm)(?:\s+(?:-C|--dir|--prefix)\s+\S+)?(?:\s+run)?\s+(?:lint|lint:fix)(?=\s|$)")
ESLINT_TREE = re.compile(r"^(?:\S*/)?eslint\b.*\s(?:\.|src|src/|playwright|playwright/|scripts|scripts/)(?:\s|$)")
VERIFY_ALL = re.compile(r"^(?:python3?\s+)?\S*verify\.py\b.*\s--all\b")
TSC = re.compile(r"^(?:(?:\S*/)?tsc(?=\s|$)|(?:pnpm|npm)(?:\s+(?:-C|--dir|--prefix)\s+\S+)?(?:\s+run)?\s+typecheck(?=\s|$))")
ESLINT = re.compile(r"^(?:\S*/)?eslint(?=\s|$)")

MAKE_FULL = {"lint", "lint-fix", "test", "test-unit", "test-integration", "test-all", "check", "check-all"}
MAKE_FULL |= {"ci", "ci-fast", "bench", "verify-all"}
MAKE_HEAVY = {"build", "build-debug", "build-release", "build-release-target", "package"}
PLAYWRIGHT_VALUE_FLAGS = {
    "--config", "-c", "--workers", "-j", "--output", "--reporter", "--timeout", "--project", "--retries",
    "--repeat-each", "--max-failures", "-x", "--grep-invert", "--trace", "--global-timeout",
}  # fmt: skip


class Finding(NamedTuple):
    full: bool
    kind: str  # rust / e2e / ts：走哪个排队槽位
    text: str


def split_commands(command: str) -> list[str]:
    """按未加引号的 ; | & ( ) 换行切开，heredoc 正文（常是脚本内容）不算命令。"""
    command = HEREDOC.sub("", command)
    parts, current, quote = [], [], None
    for char in command:
        if quote:
            current.append(char)
            if char == quote:
                quote = None
        elif char in "'\"":
            quote = char
            current.append(char)
        elif char in ";|&()\n":
            parts.append("".join(current))
            current = []
        else:
            current.append(char)
    parts.append("".join(current))
    return [WRAPPER.sub("", part.strip() + " ").strip() for part in parts if part.strip()]


def playwright_is_filtered(segment: str) -> bool:
    tokens = segment.split()
    if "test" in tokens:
        index = tokens.index("test") + 1
    else:
        index = next((i + 1 for i, t in enumerate(tokens) if t.endswith("playwright")), len(tokens))
    while index < len(tokens):
        token = tokens[index]
        if token in ("-g", "--grep", "--last-failed") or token.startswith(("--grep=", "-g=")):
            return True
        if token.startswith("-"):
            index += 2 if token in PLAYWRIGHT_VALUE_FLAGS else 1
            continue
        if token.startswith((">", "2>")):
            return False
        return True
    return False


def has_positional(arguments: str) -> bool:
    return any(not t.startswith(("-", ">", "2>")) and t != "run" for t in arguments.split())


def inspect_segment(segment: str) -> Finding | None:
    nested = SHELL_C.match(segment)
    if nested:
        found = inspect(nested.group(2))
        return found[0] if found else None

    match = CARGO.match(segment)
    if match:
        if re.search(r"\s(--help|-h|--version|-V)(\s|$)", segment):
            return None
        return Finding(bool(re.search(r"\s--(workspace|all)(\s|$)", segment)), "rust", segment)

    match = MAKE.match(segment)
    if match:
        target = match.group(1)
        if target in MAKE_FULL:
            return Finding(True, "rust", segment)
        return Finding(False, "rust", segment) if target in MAKE_HEAVY else None

    if PLAYWRIGHT.match(segment):
        if re.search(r"\s(--list|--help|-h|--version)(\s|$)", segment):
            return None
        return Finding(not playwright_is_filtered(segment), "e2e", segment)

    match = VITEST.match(segment) or PACKAGE_TEST.match(segment)
    if match:
        if re.search(r"\s--version(\s|$)", segment):
            return None
        scoped = has_positional(segment[match.end():])
        return Finding(not scoped, "ts", segment)

    if PACKAGE_LINT.match(segment) or ESLINT_TREE.match(segment):
        return Finding(True, "ts", segment)
    if TSC.match(segment) or ESLINT.match(segment):
        return None if re.search(r"\s(--version|-v|--help)(\s|$)", segment) else Finding(False, "ts", segment)
    if VERIFY_ALL.match(segment):
        return Finding(True, "verify", segment)
    return None


def inspect(command: str) -> list[Finding]:
    found = [inspect_segment(segment) for segment in split_commands(command)]
    return [f for f in found if f]


def reason_for(findings: list[Finding]) -> str:
    full = next((f for f in findings if f.full), None)
    if full:
        return (
            f"`{full.text}` 是全量验证，只在用户明确要求时才跑。平时用 `{VERIFY} <本 session 改过的文件>`，"
            f"它只跑相关检查，失败后重跑同一条命令只补失败项。用户明确要求全量时，跑 `{BYPASS} {VERIFY} --all`（可断点续跑）。"
        )

    heavy = findings[0]
    return (
        f"`{heavy.text}` 要排队跑，改成 `{VERIFY} --run -- {heavy.text}`，放后台（run_in_background）。"
        f"它负责全机排队、降优先级和共用 target/verify；不要再建私有 target。"
    )


def main() -> int:
    payload = json.load(sys.stdin)
    command = (payload.get("tool_input") or {}).get("command") or ""
    if not command or BYPASS in command:
        return 0

    findings = inspect(command)
    if findings:
        decision = {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason_for(findings),
        }
        print(json.dumps({"hookSpecificOutput": decision}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:  # noqa: BLE001 拦截只是兜底，任何异常都放行
        sys.exit(0)
