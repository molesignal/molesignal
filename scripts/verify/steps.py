"""每一步检查：看哪些文件、怎么跑、失败算谁的。按从便宜到贵排列，第一个失败就停。"""

from __future__ import annotations

import hashlib
import json
import os
import re
import socket
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from proc import Slot, artifacts_dir, cargo_env, clone_tree, new_log, run, say, web_env, web_tool
from repo import ROOT, ROOT_MANIFESTS, WEB, Workspace, common_dir, dirty_files, exists, file_hash
from store import Store, salt_for

RUST_PREFIXES = ("bin/", "crates/", "proto/", "contracts/", "conf/")
PLAYWRIGHT_ROOT = "web/playwright/"
SPEC_CHUNK = 20
DEV_SERVER_PORT = 5173
RUST_ERROR = re.compile(r"^error.*?\n\s*--> (\S+?):\d+:\d+", re.M | re.S)
TS_ERROR = re.compile(r"^(\S+?)\(\d+,\d+\): error TS\d+", re.M)
TEST_FAILURE = re.compile(r"^---- (\S+) stdout ----\n(.*?)(?=^---- |^failures:)", re.M | re.S)
ASSERT_SIDES = re.compile(r"^\s*left: (.*)\n\s*right: (.*)$", re.M)
RUST_PATH = re.compile(r"[\w./-]+\.rs\b")


@dataclass
class Result:
    ok: bool
    log: Path | None = None
    note: str = ""
    # 挡路的是别的 session 的半成品，不是你的问题。
    blocked: bool = False


class Context:
    def __init__(self, mode: str, files: list[str], specs: list[str], store: Store) -> None:
        self.mode = mode  # scoped / hook / all
        self.files = sorted(set(files))
        self.own = set(self.files)
        self.specs = specs
        self.store = store
        self._workspace: Workspace | None = None
        self._dirty: set[str] | None = None
        self._web_digest: str | None = None

    @property
    def workspace(self) -> Workspace:
        if self._workspace is None:
            self._workspace = Workspace()
        return self._workspace

    @property
    def dirty(self) -> set[str]:
        if self._dirty is None:
            self._dirty = set(dirty_files())
        return self._dirty

    def foreign_wip(self, files: set[str]) -> set[str]:
        """不在你的文件里、又还没提交：多半是别的 session 正在写。"""
        return {f for f in files if f not in self.own and f in self.dirty}

    @property
    def web_digest(self) -> str:
        """本轮前端文件内容的摘要：spec 没变但被测代码变了，也要重跑。"""
        if self._web_digest is None:
            sources = [f for f in self.files if f.startswith(("web/src/", "web/playwright/fixtures/"))]
            joined = "".join(f"{f}\0{file_hash(f)}\0" for f in sources)
            self._web_digest = hashlib.sha1(joined.encode()).hexdigest()
        return self._web_digest


# ---- 每步看哪些文件 ----


def rust_sources(ctx: Context) -> list[str]:
    return [f for f in ctx.files if f.endswith(".rs") and f.startswith(("bin/", "crates/", "tools/"))]


def rust_inputs(ctx: Context) -> list[str]:
    candidates = [f for f in ctx.files if f.startswith(RUST_PREFIXES) or f in ROOT_MANIFESTS]
    return [f for f in candidates if ctx.workspace.is_input(f)] if candidates else []


def web_code(ctx: Context) -> list[str]:
    roots = ("web/src/", "web/scripts/", PLAYWRIGHT_ROOT)
    return [f for f in ctx.files if f.startswith(roots) and f.endswith((".ts", ".tsx"))]


def eslint_inputs(ctx: Context) -> list[str]:
    return [f for f in web_code(ctx) if exists(f)]


def rule_inputs(ctx: Context) -> list[str]:
    return [f for f in ctx.files if f.startswith("web/src/") and f.endswith((".ts", ".tsx", ".json"))]


def tsc_inputs(ctx: Context) -> list[str]:
    # 比 web_code 宽：tsconfig 还 include 了 web/ 根下的配置，JSON 也参与类型检查。
    return [f for f in ctx.files if f.startswith("web/") and f.endswith((".ts", ".tsx", ".json"))]


def vitest_inputs(ctx: Context) -> list[str]:
    # i18n JSON 几乎被所有页面引用，算进去 related 就等于全量。
    return [f for f in web_code(ctx) if not f.startswith(PLAYWRIGHT_ROOT) and exists(f)]


def playwright_inputs(ctx: Context) -> list[str]:
    changed = [
        f for f in ctx.files
        if f.startswith(PLAYWRIGHT_ROOT) and f.endswith(".spec.ts") and not f.startswith(PLAYWRIGHT_ROOT + "perf/")
    ]  # fmt: skip
    return sorted({f for f in [*changed, *ctx.specs] if exists(f)})


def playwright_fingerprint(ctx: Context, path: str) -> str:
    # --all 只看 spec 本身：改一个文件不该让上千个用例全部重来。
    return file_hash(path) if ctx.mode == "all" else f"{file_hash(path)}+{ctx.web_digest}"


# ---- 每步怎么跑 ----


def run_license(_ctx: Context, pending: list[str], whole: bool) -> Result:
    # 全量时不传文件，由脚本自己扫。
    files = [] if whole else [f for f in pending if exists(f)]
    if not files and not whole:
        return Result(True)

    log = new_log("license")
    rc = run(["bash", "scripts/license-headers.sh", "--check", *files], ROOT, dict(os.environ), log)
    return Result(rc == 0, log, "" if rc == 0 else "缺版权头：make add-license-headers")


def run_rustfmt(ctx: Context, pending: list[str], whole: bool) -> Result:
    available = subprocess.run(["rustup", "run", "nightly", "rustfmt", "--version"], capture_output=True)
    if available.returncode != 0:
        note = "未检测到 nightly rustfmt：rustup toolchain install nightly --profile minimal --component rustfmt"
        return Result(False, note=note)

    log = new_log("rustfmt")
    if whole:
        rc = run(["cargo", "+nightly", "fmt", "--all", "--", "--check"], ROOT, dict(os.environ), log)
        return Result(rc == 0, log)

    files = [f for f in pending if exists(f)]
    if not files:
        return Result(True)

    # skip_children：只碰点名的文件，mod 声明指向的子模块可能是别的 session 正在写的。
    cmd = ["rustfmt", "+nightly", "--edition", "2024", "--config-path", "rustfmt.toml"]
    cmd += ["--config", "skip_children=true", "-l"]
    if ctx.mode != "scoped":
        cmd.append("--check")
    rc = run([*cmd, *files], ROOT, dict(os.environ), log)
    return Result(rc == 0, log, "" if rc == 0 else "格式不对：scripts/verify/verify.py --only rustfmt <这些文件>")


def run_eslint(ctx: Context, pending: list[str], whole: bool) -> Result:
    log = new_log("eslint")
    targets = ["."] if whole else [f[len("web/"):] for f in pending if exists(f)]
    if not targets:
        return Result(True)

    # 规则是类型感知的，一次要建整个 TS 程序，内存和 tsc 一个量级。
    with Slot("ts", "eslint"):
        rc = run([web_tool("eslint"), "--max-warnings", "0", "--no-warn-ignored", *targets], WEB, web_env(), log)
    return Result(rc == 0, log)


def _rule_tokens(ctx: Context) -> list[str]:
    tokens = []
    for path in ctx.own:
        if path.startswith("web/"):
            relative = path[len("web/"):]
            tokens.append(relative)
            # i18n 检查把 src/i18n/zh-cn/foo.json 报成 zh-cn/foo。
            match = re.match(r"src/i18n/([^/]+/[^/]+)\.json$", relative)
            if match:
                tokens.append(match.group(1))
    return tokens


def run_web_rules(ctx: Context, _pending: list[str], whole: bool) -> Result:
    """package.json 的 lint 里除 eslint 外的规则脚本。它们扫整个 src，只有点到你的文件才算你失败。"""
    script = json.loads((WEB / "package.json").read_text())["scripts"]["lint"]
    rules = [part.split()[1] for part in script.split("&&") if part.strip().startswith("tsx ")]

    tokens = _rule_tokens(ctx)
    log = new_log("web-rules")
    combined, foreign = [], 0
    for rule in rules:
        single = new_log("web-rule")
        rc = run([web_tool("tsx"), rule], WEB, web_env(), single)
        text = single.read_text(errors="ignore")
        single.unlink(missing_ok=True)
        if rc == 0:
            continue
        if whole or any(token in text for token in tokens):
            combined.append(f"== {rule}\n{text}")
        else:
            foreign += 1

    log.write_text("\n".join(combined))
    note = f"{foreign} 条规则的报错都不在你的文件里，已忽略" if foreign else ""
    return Result(not combined, log, note)


def run_tsc(ctx: Context, _pending: list[str], _whole: bool) -> Result:
    log = new_log("tsc")
    with Slot("ts", "tsc -b --noEmit"):
        rc = run([web_tool("tsc"), "-b", "--noEmit"], WEB, web_env(), log)
    if rc == 0:
        return Result(True, log)

    broken = {"web/" + path for path in TS_ERROR.findall(log.read_text(errors="ignore"))}
    if ctx.mode != "all" and broken and broken == ctx.foreign_wip(broken):
        # tsc 不会因为别处有错就少查你的文件，所以这种情况你的文件是干净的。
        return Result(True, log, f"类型错误都在别的 session 未提交的文件里，已忽略：{_short_list(broken)}")
    return Result(False, log)


def _ensure_web_dist() -> int:
    """build.rs 只要求 web/dist 存在，内容不影响 Rust 检查；重新构建反而会让 molesignal 整个重编。"""
    if (WEB / "dist" / "index.html").is_file():
        return 0

    main_dist = common_dir().parent / "web" / "dist"
    if (main_dist / "index.html").is_file():
        clone_tree(main_dist, WEB / "dist")
        return 0
    return run(["make", "web-build"], ROOT, web_env())


def _rust_failure(ctx: Context, log: Path) -> Result:
    failing = {os.path.normpath(path) for path in RUST_ERROR.findall(log.read_text(errors="ignore"))}
    if ctx.mode != "all" and failing and failing == ctx.foreign_wip(failing):
        waiting = f"编译错误都在别的 session 正在改的文件里：{_short_list(failing)}。不要替它修，等几分钟后重跑同一条命令。"
        return Result(False, log, waiting, blocked=True)
    return Result(False, log)


def _named_files(failure: str) -> set[str]:
    """一条测试失败点名的文件；断言两边是文件集合时，只有差集才是肇事者。"""
    message = re.sub(r"panicked at \S+", "", failure)
    sides = ASSERT_SIDES.search(message)
    if sides:
        return set(RUST_PATH.findall(sides.group(1))) ^ set(RUST_PATH.findall(sides.group(2)))
    return set(RUST_PATH.findall(message))


def foreign_test_failures(ctx: Context, output: str) -> list[str]:
    """失败的测试若都只点名别的 session 未提交的文件，返回这些测试；否则返回空，算你的失败。

    守卫测试（worker 登记、出站边界）扫全仓，别人的半成品会让它们失败。
    点到你的文件、已提交的文件或者什么文件都没点名，一律算你的。
    """
    failures = TEST_FAILURE.findall(output)
    failed_binaries = len(re.findall(r"^test result: FAILED", output, re.M))
    targets = re.search(r"^error: (\d+) targets? failed", output, re.M)
    # 崩溃的测试二进制没有失败明细，数目对不上就不能下结论。
    if not failures or not targets or int(targets.group(1)) != failed_binaries:
        return []

    others = [path for path in ctx.dirty if path not in ctx.own]
    names = []
    for name, body in failures:
        named = _named_files(body)
        foreign = all(any(p == f or p.endswith("/" + f) for p in others) for f in named)
        mine = any(p == f or p.endswith("/" + f) for f in named for p in ctx.own)
        if not named or not foreign or mine:
            return []
        names.append(name)
    return names


def _cargo(step: str, args: list[str], crates: set[str], ctx: Context) -> Result:
    # 只有 molesignal 的 build.rs 要嵌入前端产物。
    if "molesignal" in crates and _ensure_web_dist() != 0:
        return Result(False, note="web/dist 不存在且构建失败：make web-build")

    log = new_log(step)
    with Slot("rust", "cargo " + " ".join(args)):
        rc = run(["cargo", *args], ROOT, cargo_env(), log)
    return Result(True, log) if rc == 0 else _rust_failure(ctx, log)


def run_clippy(ctx: Context, pending: list[str], whole: bool) -> Result:
    workspace = ctx.workspace
    crates = set(workspace.crates) if whole else workspace.with_dependents(workspace.changed(pending))
    if not crates:
        return Result(True)

    selection = ["--workspace"] if whole else workspace.package_args(crates)
    return _cargo("clippy", ["clippy", *selection, "--all-targets", "--", "-D", "warnings"], crates, ctx)


def run_cargo_test(ctx: Context, pending: list[str], whole: bool) -> Result:
    workspace = ctx.workspace
    crates = set(workspace.crates) if whole else workspace.to_test(pending)
    if not crates:
        return Result(True)

    # 不停在第一个失败的 crate：一轮看全所有失败，少跑几轮。
    if whole:
        args = ["test", "--workspace", "--lib", "--bins", "--no-fail-fast"]
    else:
        args = ["test", *workspace.package_args(crates), "--lib", "--no-fail-fast"]

    result = _cargo("cargo-test", args, crates, ctx)
    if result.ok or result.blocked or whole or ctx.mode == "all" or not result.log:
        return result

    foreign = foreign_test_failures(ctx, result.log.read_text(errors="ignore"))
    if foreign:
        note = f"{len(foreign)} 个测试失败都只点名别的 session 未提交的文件，已忽略：{_short_list(set(foreign))}"
        return Result(True, result.log, note)
    return result


def run_vitest(ctx: Context, pending: list[str], whole: bool) -> Result:
    salt = salt_for("web")
    retry = [f for f in ctx.store.retry("vitest", pending, salt) if (WEB / f).is_file()]
    report = artifacts_dir("logs") / f"vitest-{os.getpid()}.json"
    if retry:
        say(f"vitest 上次有 {len(retry)} 个测试文件失败，只重跑这些")
        selection = ["run", *retry]
    elif whole:
        selection = ["run"]
    else:
        selection = ["related", *[f[len("web/"):] for f in pending], "--run", "--passWithNoTests"]

    workers = "4" if whole and not retry else "2"
    cmd = [web_tool("vitest"), *selection, f"--maxWorkers={workers}", "--testTimeout=30000"]
    cmd += ["--reporter=default", "--reporter=json", f"--outputFile.json={report}"]
    log = new_log("vitest")
    # 全量要跑很久，走 e2e 槽位，不堵别的 session 的 tsc。
    with Slot("e2e" if whole and not retry else "ts", "vitest"):
        rc = run(cmd, WEB, web_env(), log)

    failed: list[str] = []
    try:
        results = json.loads(report.read_text()).get("testResults", [])
        failed = [os.path.relpath(r["name"], WEB) for r in results if r.get("status") == "failed"]
    except (OSError, ValueError):
        pass
    report.unlink(missing_ok=True)

    ctx.store.set_retry("vitest", pending, salt, [] if rc == 0 else failed)
    return Result(rc == 0, log)


def _failed_specs(report: Path) -> set[str] | None:
    """这批里失败的 spec 文件；报告不完整（进程被杀、配置错误）时返回 None，整批不算通过。"""
    try:
        data = json.loads(report.read_text())
    except (OSError, ValueError):
        return None

    root = data.get("config", {}).get("rootDir") or str(WEB / "playwright")
    failed: set[str] = set()

    def walk(suite: dict) -> None:
        for spec in suite.get("specs", []):
            if not spec.get("ok", False):
                failed.add(os.path.relpath(os.path.join(root, spec["file"]), ROOT))
        for child in suite.get("suites", []):
            walk(child)

    for suite in data.get("suites", []):
        walk(suite)
    return failed


def _serves_other_checkout() -> bool:
    """worktree 里，:5173 上的 dev server 属于主 checkout，spec 测到的就不是这份代码。"""
    if common_dir().parent == ROOT:
        return False
    try:
        socket.create_connection(("localhost", DEV_SERVER_PORT), timeout=1).close()
        return True
    except OSError:
        return False


def run_playwright(ctx: Context, pending: list[str], whole: bool) -> Result:
    """分批跑，每批结束就记下通过的 spec：中断或失败后，下次只跑没通过的 spec 文件。"""
    if _serves_other_checkout():
        note = (
            f"这是 worktree，:{DEV_SERVER_PORT} 的 dev server 属于主 checkout，跑出来的结果不算数。"
            "用改了端口的 Playwright 配置：verify.py --run -- npx playwright test --config=<配置> <spec>"
        )
        return Result(False, note=note)

    workers = "4" if whole else "2"
    output = artifacts_dir("pw") / f"run-{os.getpid()}"
    log = new_log("playwright")
    report = output.with_suffix(".json")
    salt = salt_for("web")
    failed: set[str] = set()

    with open(log, "ab") as combined:
        for start in range(0, len(pending), SPEC_CHUNK):
            chunk = pending[start:start + SPEC_CHUNK]
            cmd = [web_tool("playwright"), "test", *[f[len("web/"):] for f in chunk]]
            cmd += ["--grep-invert", "@perf", f"--workers={workers}", f"--output={output}"]
            # 不写基线：缺基线就是失败，重新截图要用户明确要求。
            cmd += ["--update-snapshots=none", "--reporter=line,json"]
            # 跑之前取指纹：跑的过程中 spec 被改，就不算验过改后的内容。
            prints = {spec: playwright_fingerprint(ctx, spec) for spec in chunk}
            env = web_env()
            env["PLAYWRIGHT_JSON_OUTPUT_NAME"] = str(report)

            # 一批一抢槽位：全量跑半小时，也不会把别的 session 的三个 spec 堵到最后。
            chunk_log = new_log("playwright-chunk")
            with Slot("e2e", f"playwright {len(chunk)} 个 spec"):
                rc = run(cmd, WEB, env, chunk_log)
            combined.write(chunk_log.read_bytes())
            combined.flush()
            chunk_log.unlink(missing_ok=True)

            bad = _failed_specs(report)
            report.unlink(missing_ok=True)
            if bad is None or (rc != 0 and not bad):
                return Result(False, log, "Playwright 没有产出完整报告（配置错误或被中断），这批不计通过")

            ctx.store.mark("playwright", {s: value for s, value in prints.items() if s not in bad}, salt)
            failed |= bad
            say(f"playwright {min(start + SPEC_CHUNK, len(pending))}/{len(pending)} 个 spec，失败 {len(failed)} 个")

    return Result(not failed, log, f"失败的 spec：{_short_list(failed)}" if failed else "")


def _short_list(paths: set[str], limit: int = 6) -> str:
    listed = sorted(paths)
    return "、".join(listed[:limit]) + (f" 等 {len(listed)} 个" if len(listed) > limit else "")


@dataclass
class Step:
    name: str
    domain: str
    inputs: Callable[[Context], list[str]]
    run: Callable[[Context, list[str], bool], Result]
    fingerprint: Callable[[Context, str], str] = lambda _ctx, path: file_hash(path)


STEPS = [
    Step("license", "rust", rust_sources, run_license),
    Step("rustfmt", "rust", rust_sources, run_rustfmt),
    Step("eslint", "web", eslint_inputs, run_eslint),
    Step("web-rules", "web", rule_inputs, run_web_rules),
    Step("tsc", "web", tsc_inputs, run_tsc),
    Step("clippy", "rust", rust_inputs, run_clippy),
    Step("vitest", "web", vitest_inputs, run_vitest),
    Step("cargo-test", "rust", rust_inputs, run_cargo_test),
    Step("playwright", "web", playwright_inputs, run_playwright, playwright_fingerprint),
]
# 提交时只挡“这次提交本身就不对”的问题；测试在提交前的验证里跑过了。
HOOK_STEPS = ("license", "rustfmt", "tsc", "clippy")
