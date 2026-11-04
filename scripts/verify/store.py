"""记住哪一步在哪些文件内容上通过过。

按文件记，而不是按整棵树记：共用工作树里别的 session 一直在改文件，整棵树的哈希永远对不上。
也因此这里只回答“你的文件验过没有”，跨 session 的集成问题留给 --all 和 CI。
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import os

from repo import exists, file_hash, git_dir

# 这些文件一变，对应一类步骤的旧记录全部作废。
SALT_FILES = {
    "rust": ["Cargo.lock", "rust-toolchain.toml", "clippy.toml", "rustfmt.toml", ".cargo/config.toml"],
    "web": [
        "web/package.json",
        "web/pnpm-lock.yaml",
        "web/tsconfig.json",
        "web/eslint.config.mjs",
        "web/vitest.config.ts",
        "web/vitest.setup.ts",
        "web/playwright.config.ts",
    ],
}
MAX_RETRY_ENTRIES = 20


def salt_for(domain: str) -> str:
    return hashlib.sha1("".join(file_hash(f) for f in SALT_FILES[domain] if exists(f)).encode()).hexdigest()


def retry_key(pending: list[str]) -> str:
    return hashlib.sha1("\0".join(sorted(pending)).encode()).hexdigest()


class Store:
    """mode=scoped 的记录各 session 共享：同样内容的文件别人验过，你就不用再验。"""

    def __init__(self, namespace: str) -> None:
        directory = git_dir() / "verify"
        directory.mkdir(parents=True, exist_ok=True)
        self.path = directory / f"state-{namespace}.json"
        self.lock = directory / f"state-{namespace}.lock"

    def _load(self) -> dict:
        try:
            return json.loads(self.path.read_text())
        except (FileNotFoundError, ValueError):
            return {}

    def _update(self, change) -> None:
        with open(self.lock, "a") as handle:
            fcntl.flock(handle, fcntl.LOCK_EX)
            data = self._load()
            change(data)
            temporary = self.path.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_text(json.dumps(data))
            os.replace(temporary, self.path)

    def _record(self, data: dict, step: str, salt: str) -> dict:
        record = data.get(step)
        if not record or record.get("salt") != salt:
            record = {"salt": salt, "files": {}, "retry": {}}
            data[step] = record
        return record

    def pending(self, step: str, prints: dict[str, str], salt: str) -> list[str]:
        record = self._load().get(step) or {}
        passed = record.get("files", {}) if record.get("salt") == salt else {}
        return [path for path, value in prints.items() if passed.get(path) != value]

    def mark(self, step: str, prints: dict[str, str], salt: str) -> None:
        self._update(lambda data: self._record(data, step, salt)["files"].update(prints))

    def retry(self, step: str, pending: list[str], salt: str) -> list[str]:
        record = self._load().get(step) or {}
        if record.get("salt") != salt:
            return []
        return record.get("retry", {}).get(retry_key(pending), [])

    def set_retry(self, step: str, pending: list[str], salt: str, files: list[str]) -> None:
        def change(data: dict) -> None:
            retry = self._record(data, step, salt)["retry"]
            retry.pop(retry_key(pending), None)
            if files:
                retry[retry_key(pending)] = files
            for stale in list(retry)[:-MAX_RETRY_ENTRIES]:
                del retry[stale]

        self._update(change)

    def clear(self) -> None:
        self.path.unlink(missing_ok=True)
