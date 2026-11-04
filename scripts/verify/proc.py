"""跑子进程：全机排队、降优先级、日志落盘，被杀时把子进程一起收走。"""

from __future__ import annotations

import fcntl
import json
import os
import platform
import shutil
import signal
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

from repo import ROOT, WEB, common_dir, git_dir

STEP_TIMEOUT_SECONDS = 2 * 60 * 60
ARTIFACT_KEEP_SECONDS = 3 * 24 * 60 * 60
QUEUE_NOTICE_SECONDS = 60
_child: subprocess.Popen | None = None


def say(message: str) -> None:
    print(f"[verify] {message}", flush=True)


def _on_signal(signum: int, _frame: object) -> None:
    if _child is not None and _child.poll() is None:
        os.killpg(_child.pid, signal.SIGTERM)
    sys.exit(128 + signum)


def install_signal_handlers() -> None:
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, _on_signal)


class Slot:
    """同一台机器同一时间只跑一个 rust（cargo）、一个 e2e（Playwright）、一个 ts（tsc、eslint、vitest）。

    真正的瓶颈是内存：24 GB 的机器上几份 rustc 同编 molesignal 就会把交换区用满，
    进程卡在换页上，负载上百而 CPU 反倒很低。槽位按内存大户分，三个槽位同时满也放得下。
    flock 在进程退出时由内核释放，崩溃不会留下死锁。
    """

    def __init__(self, name: str, label: str) -> None:
        self.name = name
        self.label = label

    def __enter__(self) -> "Slot":
        self.handle = _open_slot(self.name)
        if self.handle is None:
            say(f"{self.name} 槽位不可写，这一步不排队")
            return self

        waited = 0.0
        while True:
            try:
                fcntl.flock(self.handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if waited % QUEUE_NOTICE_SECONDS == 0:
                    say(f"排队等 {self.name} 槽位：{describe_holder(self.handle)}")
                time.sleep(2)
                waited += 2

        self.handle.seek(0)
        self.handle.truncate()
        json.dump({"pid": os.getpid(), "label": self.label, "cwd": os.getcwd(), "since": time.time()}, self.handle)
        self.handle.flush()
        return self

    def __exit__(self, *_exc: object) -> None:
        if self.handle is None:
            return

        self.handle.seek(0)
        self.handle.truncate()
        self.handle.flush()
        fcntl.flock(self.handle, fcntl.LOCK_UN)
        self.handle.close()


def _open_slot(name: str):
    """全机一份，别的仓库的验证也排同一条队：抢的是同一块内存。目录不可写（沙箱）时退回本仓库的队列。"""
    shared = Path(os.environ.get("MS_VERIFY_SLOTS") or Path.home() / ".cache" / "verify-slots")
    for directory in (shared, common_dir() / "verify" / "slots"):
        try:
            directory.mkdir(parents=True, exist_ok=True)
            return open(directory / f"{name}.lock", "a+")
        except OSError:
            continue
    return None


def describe_holder(handle) -> str:
    handle.seek(0)
    try:
        holder = json.loads(handle.read() or "{}")
    except ValueError:
        holder = {}
    if not holder:
        return "占用者信息未知"
    minutes = int((time.time() - holder.get("since", time.time())) / 60)
    project = Path(holder.get("cwd", "?")).name
    return f"{project} 的 pid {holder.get('pid')} 在跑 {holder.get('label')}（已 {minutes} 分钟）"


def slot_status() -> list[str]:
    lines = []
    for name in ("rust", "e2e", "ts"):
        handle = _open_slot(name)
        if handle is None:
            lines.append(f"{name}: 不可写")
            continue

        with handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_SH | fcntl.LOCK_NB)
                fcntl.flock(handle, fcntl.LOCK_UN)
                lines.append(f"{name}: 空闲")
            except BlockingIOError:
                lines.append(f"{name}: {describe_holder(handle)}")
    return lines


def artifacts_dir(kind: str) -> Path:
    directory = git_dir() / "verify" / kind
    directory.mkdir(parents=True, exist_ok=True)

    for entry in directory.iterdir():
        try:
            if time.time() - entry.stat().st_mtime <= ARTIFACT_KEEP_SECONDS:
                continue
            if entry.is_dir():
                shutil.rmtree(entry, ignore_errors=True)
            else:
                entry.unlink(missing_ok=True)
        except FileNotFoundError:
            # 别的进程同时在清理。
            continue
    return directory


def new_log(name: str) -> Path:
    return artifacts_dir("logs") / f"{name}-{datetime.now():%Y%m%d-%H%M%S}-{os.getpid()}.log"


def run(cmd: list[str], cwd: Path, env: dict[str, str], log: Path | None = None) -> int:
    """低优先级跑一条命令；log 为空时输出直接给调用方。"""
    global _child
    output = open(log, "wb") if log else None
    _child = subprocess.Popen(
        ["nice", "-n", "10", *cmd],
        cwd=cwd,
        env=env,
        stdout=output,
        stderr=subprocess.STDOUT if output else None,
        start_new_session=True,
    )

    try:
        return _child.wait(timeout=STEP_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        os.killpg(_child.pid, signal.SIGTERM)
        _child.wait()
        return 124
    finally:
        _child = None
        if output:
            output.close()


def clone_tree(source: Path, target: Path) -> None:
    """APFS 上写时复制，几乎不占盘；其他系统老实复制。"""
    target.parent.mkdir(parents=True, exist_ok=True)
    clone = ["cp", "-c", "-R"] if platform.system() == "Darwin" else ["cp", "-R"]
    temporary = target.with_name(target.name + ".cloning")
    shutil.rmtree(temporary, ignore_errors=True)
    subprocess.run([*clone, str(source), str(temporary)], check=True)
    os.replace(temporary, target)


def build_jobs() -> str:
    # 留出核给 dev 后端、Vite 和另一个槽位；cargo 默认会吃满全部核。
    return os.environ.get("MS_VERIFY_JOBS") or str(max(2, (os.cpu_count() or 4) * 6 // 10))


def _bootstrap_verify_target(target: Path) -> None:
    """第一次用 target/verify 时从主 target 克隆，免去第三方依赖的冷编译。"""
    if (target / "debug").exists():
        return

    main_root = common_dir().parent
    for source in (ROOT / "target" / "debug", main_root / "target" / "debug"):
        if source.is_dir() and platform.system() == "Darwin":
            say(f"首次使用 {target}：从 {source} 写时复制依赖产物")
            clone_tree(source, target / "debug")
            return


def cargo_env(*, this_checkout: bool = True) -> dict[str, str]:
    """验证用的 cargo 共用一个 target/verify：不抢 make run 的构建锁，也不用每个 session 冷编译一份。

    每个 checkout 各有自己的 target/，所以不会出现两份源码共用一个 target 的问题。
    首次会克隆，必须在持有 rust 槽位时调用。
    """
    env = dict(os.environ)
    env["CARGO_BUILD_JOBS"] = build_jobs()
    if this_checkout and "CARGO_TARGET_DIR" not in env:
        target = ROOT / "target" / "verify"
        _bootstrap_verify_target(target)
        env["CARGO_TARGET_DIR"] = str(target)
    return env


def web_env() -> dict[str, str]:
    """vitest 只在 Node 22 下能过（Homebrew 的 Node 26 里 jsdom 没有 localStorage）。"""
    env = dict(os.environ)
    candidates = sorted(Path.home().glob(".nvm/versions/node/v22.*/bin"))
    if candidates:
        env["PATH"] = f"{candidates[-1]}{os.pathsep}{env.get('PATH', '')}"
    return env


def web_tool(name: str) -> str:
    return str(WEB / "node_modules" / ".bin" / name)
