#!/usr/bin/env python3
"""按改动范围验证：只跑和这些文件有关的检查，通过的不再跑，重型步骤全机排队。

    scripts/verify/verify.py <本 session 改过的文件...> [--specs a.spec.ts ...]
        失败或中断后重跑同一条命令：只补失败项和文件有变化的步骤。
    scripts/verify/verify.py --all
        全量门禁，只在用户明确要求时跑；同样能续跑，--fresh 从头开始。
    scripts/verify/verify.py --run -- <命令>
        单条诊断命令（单个测试、单个 crate）：排队、降优先级、共用 target/verify。
    scripts/verify/verify.py --status
        看谁占着排队槽位。

退出码：0 通过；1 失败；2 用法错误；3 被别的 session 的半成品挡住，等一会儿重跑。
"""

from __future__ import annotations

import argparse
import os
import shlex
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import guard  # noqa: E402
from proc import (  # noqa: E402
    Slot,
    artifacts_dir,
    cargo_env,
    clone_tree,
    install_signal_handlers,
    run,
    say,
    slot_status,
    web_env,
)
from repo import (  # noqa: E402
    ROOT,
    WEB,
    Blocked,
    all_files,
    common_dir,
    dirty_files,
    expand,
    is_this_checkout,
    normalize,
    staged_files,
)
from steps import HOOK_STEPS, STEPS, Context, Result  # noqa: E402
from store import Store, salt_for  # noqa: E402

BYPASS_ENV = "MS_VERIFY_BYPASS"
# 共用工作树里 git status 能有上千条，绝大多数是别的 session 的：这时必须让调用方点名。
DIRTY_LIMIT = 200
FAILURE_TAIL_LINES = 60


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="*", help="本 session 改过的文件")
    parser.add_argument("--files-from", help="从文件读路径，一行一个；- 表示标准输入")
    parser.add_argument("--specs", nargs="+", default=[], help="要跑的 Playwright spec，可只写文件名")
    parser.add_argument("--only", help="只跑这些步骤，逗号分隔")
    parser.add_argument("--skip", help="跳过这些步骤，逗号分隔")
    parser.add_argument("--all", action="store_true", help="全量门禁")
    parser.add_argument("--fresh", action="store_true", help="清掉通过记录，从头开始")
    parser.add_argument("--hook", action="store_true", help="pre-commit 用：只查暂存文件")
    parser.add_argument("--status", action="store_true", help="看排队槽位")
    return parser.parse_args(argv)


def resolve_spec(name: str) -> str | None:
    candidates = [name, f"{name}.spec.ts"]
    for candidate in candidates:
        for prefix in ("", "web/", "web/playwright/", "web/playwright/tests/"):
            if (ROOT / prefix / candidate).is_file():
                return f"{prefix}{candidate}"
    return normalize(name) if Path(name).is_file() else None


def gather_files(args: argparse.Namespace) -> list[str] | None:
    if args.all:
        return all_files()
    if args.hook:
        return staged_files()

    named = list(args.files)
    if args.files_from:
        source = sys.stdin if args.files_from == "-" else open(args.files_from)
        named += [line.strip() for line in source if line.strip()]
    if named:
        return expand([path for path in map(normalize, named) if path])

    # 只点名了 spec，就只验 spec，不要回退去扫 git status。
    if args.specs:
        return []
    dirty = dirty_files()
    if len(dirty) > DIRTY_LIMIT:
        say(f"工作区有 {len(dirty)} 个改动，多半含别的 session 的半成品。把本 session 改过的文件列出来再跑。")
        return None
    return dirty


def chosen_steps(args: argparse.Namespace) -> list | None:
    wanted = set(args.only.split(",")) if args.only else set()
    unwanted = set(args.skip.split(",")) if args.skip else set()
    unknown = (wanted | unwanted) - {s.name for s in STEPS}
    if unknown:
        say(f"没有这些步骤：{'、'.join(sorted(unknown))}。可选：{'、'.join(s.name for s in STEPS)}")
        return None

    steps = [s for s in STEPS if s.name in HOOK_STEPS] if args.hook else STEPS
    return [s for s in steps if (not wanted or s.name in wanted) and s.name not in unwanted]


def report_failure(step_name: str, result: Result, seconds: float, mode: str) -> None:
    say(f"✗ {step_name}（{seconds:.0f}s）")
    if result.log and result.log.is_file():
        lines = result.log.read_text(errors="ignore").splitlines()
        print("\n".join(lines[-FAILURE_TAIL_LINES:]), flush=True)
        say(f"完整日志：{result.log}")
    if result.note:
        say(result.note)
    if not result.blocked:
        say("修完后重新提交。" if mode == "hook" else "修完后重跑同一条命令：已通过的步骤不会再跑。")


def verify(args: argparse.Namespace) -> int:
    files = gather_files(args)
    steps = chosen_steps(args)
    if files is None or steps is None:
        return 2

    specs: list[str] = []
    for name in args.specs:
        resolved = resolve_spec(name)
        if not resolved:
            say(f"找不到 spec：{name}")
            return 2
        specs.append(resolved)

    store = Store("all" if args.all else "scoped")
    if args.fresh:
        store.clear()

    mode = "all" if args.all else "hook" if args.hook else "scoped"
    ctx = Context(mode, files, specs, store)
    ran = 0
    for step in steps:
        inputs = step.inputs(ctx)
        if not inputs:
            continue
        if step.domain == "web" and not (WEB / "node_modules" / ".bin").is_dir():
            say("没有前端依赖，先运行：pnpm -C web install --frozen-lockfile")
            return 2

        salt = salt_for(step.domain)
        prints = {path: step.fingerprint(ctx, path) for path in inputs}
        pending = store.pending(step.name, prints, salt)
        if not pending:
            say(f"= {step.name} 已通过，跳过")
            continue

        ran += 1
        whole = mode == "all" and len(pending) == len(inputs)
        scope = "全量" if whole else f"{len(pending)} 个文件"
        say(f"→ {step.name}（{scope}）")
        started = time.time()
        result = step.run(ctx, pending, whole)
        seconds = time.time() - started

        if not result.ok:
            report_failure(step.name, result, seconds, mode)
            return 3 if result.blocked else 1

        # 格式化会改文件内容，按写回后的内容记。
        if step.name == "rustfmt":
            prints = {path: step.fingerprint(ctx, path) for path in inputs}
        store.mark(step.name, {path: prints[path] for path in pending}, salt)
        say(f"✓ {step.name}（{seconds:.0f}s）{result.note}")

    if ran == 0 and mode != "hook":
        say("没有需要跑的检查。")
    if mode == "scoped" and not args.only and not specs and any(f.startswith("web/src/") for f in files):
        if not any(f.endswith(".spec.ts") for f in files):
            say("提示：改了 web/src 但没有选 Playwright spec。需要时用 --specs 点名覆盖这个页面的 spec。")
    return 0


def ensure_node_modules() -> None:
    """新 worktree 没有 node_modules，从主 checkout 写时复制。"""
    main_modules = common_dir().parent / "web" / "node_modules"
    if not (WEB / "node_modules").is_dir() and main_modules.is_dir():
        clone_tree(main_modules, WEB / "node_modules")


def run_passthrough(command: list[str]) -> int:
    """单条诊断命令：不记状态，只负责排队和环境。"""
    if not command:
        say("--run 后面要跟命令，例如 --run -- cargo test -p signals --lib")
        return 2

    findings = guard.inspect(shlex.join(command))
    if any(f.full for f in findings) and os.environ.get(BYPASS_ENV) != "1":
        say(guard.reason_for(findings))
        return 2

    kinds = {f.kind for f in findings}
    if "rust" in kinds:
        with Slot("rust", shlex.join(command)[:120]):
            return run(command, Path.cwd(), cargo_env(this_checkout=is_this_checkout(Path.cwd())))

    if "e2e" in kinds:
        joined = " ".join(command)
        if "--workers" not in joined:
            command = [*command, "--workers=2"]
        if "--output" not in joined:
            command = [*command, f"--output={artifacts_dir('pw') / f'run-{os.getpid()}'}"]
        with Slot("e2e", shlex.join(command)[:120]):
            return run(command, Path.cwd(), web_env())

    if "ts" in kinds:
        with Slot("ts", shlex.join(command)[:120]):
            return run(command, Path.cwd(), web_env())

    return run(command, Path.cwd(), dict(os.environ))


def main(argv: list[str]) -> int:
    install_signal_handlers()
    if "--run" in argv:
        index = argv.index("--run")
        command = argv[index + 1:]
        return run_passthrough(command[1:] if command[:1] == ["--"] else command)

    args = parse_args(argv)
    if args.status:
        print("\n".join(slot_status()))
        return 0

    try:
        ensure_node_modules()
        return verify(args)
    except Blocked as blocked:
        say(str(blocked))
        return 3


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
