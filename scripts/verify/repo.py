"""这棵工作树的事实：有哪些改动、workspace 里谁依赖谁、改了什么要测哪些 crate。"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "web"

ROOT_MANIFESTS = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "clippy.toml", ".cargo/config.toml"}
# crate 目录之外、但会进 crate 构建或测试的路径。
OUTSIDE_INPUTS = {
    "proto/": ("protocol", "probe-agent"),
    "contracts/": ("contracts", "query-language"),
    "conf/": ("settings",),
}
# 这两个 crate 的测试读全仓 Rust 源码（worker 登记、出站边界、指标进内置大盘），
# 只靠 crate 依赖关系选不到，任何 Rust 改动都要带上；没动过时测试二进制是现成的，几乎不花时间。
GUARD_CRATES = ("signals", "postgres")
# molesignal 的语言包测试读基线 SQL。
EXTRA_TESTED = {"crates/engines/postgres/src/migrations/": ("molesignal",)}


class Blocked(Exception):
    """工作树暂时验证不了（比如别的 session 把 Cargo.toml 改到一半）。"""


def git(*args: str) -> str:
    done = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    if done.returncode != 0:
        raise Blocked(f"git {' '.join(args)} 失败：{done.stderr.strip()}")
    return done.stdout


def git_dir() -> Path:
    return Path(git("rev-parse", "--absolute-git-dir").strip())


def common_dir() -> Path:
    """所有 worktree 共用的 .git；排队槽位放这里，才是全机一份。"""
    return Path(git("rev-parse", "--path-format=absolute", "--git-common-dir").strip())


def staged_files() -> list[str]:
    return [p for p in git("diff", "--cached", "--name-only", "--no-renames", "-z").split("\0") if p]


def dirty_files() -> list[str]:
    # 不拿 index 锁：共用工作树里别的 session 可能正在 git add。
    out = git("--no-optional-locks", "status", "--porcelain", "-z", "--no-renames", "-uall")
    return [entry[3:] for entry in out.split("\0") if len(entry) > 3]


def all_files(*paths: str) -> list[str]:
    arguments = ["ls-files", "-z", "-c", "-o", "--exclude-standard"]
    return [p for p in git(*arguments, "--", *paths).split("\0") if p]


def expand(paths: list[str]) -> list[str]:
    """目录展开成里面的文件：传目录比一个个列文件方便，也不该静默什么都不查。"""
    directories = [p for p in paths if (ROOT / p).is_dir()]
    files = [p for p in paths if not (ROOT / p).is_dir()]
    return files + (all_files(*directories) if directories else [])


def is_this_checkout(directory: Path) -> bool:
    """directory 属于这份源码吗。.claude/worktrees/x 虽在 ROOT 之下，却是另一份源码。"""
    done = subprocess.run(["git", "-C", str(directory), "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    return done.returncode == 0 and Path(done.stdout.strip()).resolve() == ROOT


def normalize(path: str) -> str | None:
    """命令行给的路径转成相对仓库根；session 的 cwd 可能在 web/ 下，两种写法都接受。"""
    if os.path.isabs(path):
        relative = os.path.relpath(path, ROOT)
    elif os.path.exists(path):
        relative = os.path.relpath(os.path.abspath(path), ROOT)
    elif (ROOT / path).exists():
        relative = os.path.normpath(path)
    else:
        # 已删除的文件：优先按 cwd 解释。
        relative = os.path.relpath(os.path.abspath(path), ROOT)
    return None if relative.startswith("..") else relative


def file_hash(path: str) -> str:
    try:
        return hashlib.sha1((ROOT / path).read_bytes()).hexdigest()
    except (FileNotFoundError, IsADirectoryError):
        return "-"


def exists(path: str) -> bool:
    return (ROOT / path).is_file()


@dataclass(frozen=True)
class Crate:
    name: str
    version: str
    directory: str
    dependencies: frozenset

    @property
    def spec(self) -> str:
        # 只写名字会和同名第三方 crate 冲突（如 DNS 的 domain），带上版本。
        return f"{self.name}@{self.version}"


class Workspace:
    def __init__(self) -> None:
        done = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        if done.returncode != 0:
            raise Blocked(f"cargo metadata 失败，可能有 session 正在改 Cargo.toml：\n{done.stderr.strip()}")

        self.crates: dict[str, Crate] = {}
        for package in json.loads(done.stdout)["packages"]:
            directory = os.path.relpath(Path(package["manifest_path"]).parent, ROOT)
            dependencies = frozenset(d["name"] for d in package["dependencies"] if d.get("path"))
            self.crates[package["name"]] = Crate(package["name"], package["version"], directory, dependencies)

    def owner(self, path: str) -> Crate | None:
        owners = [c for c in self.crates.values() if path.startswith(c.directory + "/")]
        return max(owners, key=lambda c: len(c.directory)) if owners else None

    def is_input(self, path: str) -> bool:
        if path.endswith(".md"):
            return False
        if path in ROOT_MANIFESTS or path.startswith(tuple(OUTSIDE_INPUTS)):
            return True
        return self.owner(path) is not None

    def changed(self, files: list[str]) -> set[str]:
        """这些文件直接影响到的 crate。"""
        names: set[str] = set()
        for path in files:
            crate = self.owner(path)
            if crate:
                names.add(crate.name)
            if path in ROOT_MANIFESTS:
                # 根清单影响全部；molesignal 是总装 crate，依赖几乎所有成员，查它代价最小。
                names.add("molesignal")
            for prefix, crates in OUTSIDE_INPUTS.items():
                if path.startswith(prefix):
                    names.update(crates)
        return names & set(self.crates)

    def with_dependents(self, names: set[str]) -> set[str]:
        """改接口会弄坏下游，所以 clippy 要带上依赖它们的 crate。"""
        selected = set(names)
        grew = True
        while grew:
            grew = False
            for crate in self.crates.values():
                if crate.name not in selected and crate.dependencies & selected:
                    selected.add(crate.name)
                    grew = True
        return selected

    def to_test(self, files: list[str]) -> set[str]:
        names = self.changed(files)
        if any(path.endswith(".rs") for path in files):
            names.update(GUARD_CRATES)
        for prefix, crates in EXTRA_TESTED.items():
            if any(path.startswith(prefix) for path in files):
                names.update(crates)
        return names & set(self.crates)

    def package_args(self, names: set[str]) -> list[str]:
        args: list[str] = []
        for name in sorted(names):
            args += ["-p", self.crates[name].spec]
        return args
