"""Delete the rustc object files in a cargo target directory that nothing will read again.

On macOS, dev builds use split-debuginfo=unpacked: rustc leaves every codegen-unit
object in deps/ next to the unit's output, as <crate>-<hash>.<cgu>.rcgu.o, because a
linked binary keeps no debug info of its own. Its debug map points at the loose
objects of its own unit and at the members inside the rlibs of its dependencies.
Incremental compilation names the objects anew on every compile, so each recompile
of a workspace unit adds a full set (about 2 GB for the molesignal test binary) and
cargo never deletes the old ones: target/verify had reached 291 GB on 2026-11-10,
and target/debug, which make run builds, fills up the same way.

An object is still read only
- by the compile that writes it, until it has archived or linked it;
- through the debug map of its own unit's executable or dylib, if it came from the
  compile that linked that file.
The loose objects of an rlib unit are copies of the rlib's members, and incremental
compilation keeps its own links under incremental/, so nothing reads those again.

Time tells both cases apart, with GRACE_SECONDS of margin. One compile of molesignal
writes its objects within about 20 minutes before it links. The inode change time is
the one to compare: an object reused from incremental/ is hard-linked under its new
name, which keeps the old modification time.

Pruning whole units is out of scope on purpose: a dependency that is only read when
a build script or proc macro recompiles looks unused for days, and deleting it
rebuilds hundreds of crates.

verify.py prunes target/verify after every cargo command it runs. Run as a script,
this module prunes one profile directory: make build-debug runs it after the build,
and make run starts it in the background with --wait, so that it looks once cargo
has built and the server starts no later than before.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import re
import stat
import sys
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

GRACE_SECONDS = 60 * 60
OBJECT_SUFFIX = ".rcgu.o"
# Names in deps/ start with the crate name, which has no dash, and the unit hash.
UNIT_NAME = re.compile(r"^[^-]+-([0-9a-f]{16})(\.|$)")
LINKED_SUFFIXES = (".dylib", ".so")
# While cargo builds into a profile directory it holds some of these exclusively (cargo
# 1.96: the first two); `cargo run` lets go of them before it starts the program.
CARGO_LOCKS = (".cargo-build-lock", ".cargo-artifact-lock", ".cargo-lock")
# How long a cargo started next to `prune.py --wait` gets to take those locks.
SETTLE_SECONDS = 5


@dataclass
class Pruned:
    files: int = 0
    # Counted only when the last link of a file goes; a copy in incremental/ keeps it alive.
    bytes: int = 0


def stale_objects(files: dict[str, os.stat_result], now: float) -> list[str]:
    """The objects in one deps/ listing that nothing will read again."""
    objects: dict[str, list[str]] = {}
    linked_at: dict[str, float] = {}
    for name, info in files.items():
        match = UNIT_NAME.match(name)
        if not match:
            continue

        unit = match.group(1)
        if name.endswith(OBJECT_SUFFIX):
            objects.setdefault(unit, []).append(name)
        elif match.group(2) == "" or name.endswith(LINKED_SUFFIXES):
            # An executable has no extension; a proc macro or dylib does.
            linked_at[unit] = max(linked_at.get(unit, 0.0), info.st_mtime)

    stale = []
    for unit, names in objects.items():
        for name in names:
            changed = files[name].st_ctime
            if changed > now - GRACE_SECONDS:
                continue
            if unit in linked_at and changed >= linked_at[unit] - GRACE_SECONDS:
                continue
            stale.append(name)
    return sorted(stale)


def prune_objects(target: Path, now: float | None = None) -> Pruned:
    """Delete the stale objects in every <profile>/deps/ of a cargo target directory.

    Safe while another compile writes to the same directory: its objects are younger
    than GRACE_SECONDS.
    """
    return _prune(sorted(target.glob("*/deps")), now)


def prune_profile(profile: Path, now: float | None = None) -> Pruned:
    """The same for one profile directory, such as target/debug."""
    return _prune([profile / "deps"], now)


def _prune(directories: list[Path], now: float | None) -> Pruned:
    now = time.time() if now is None else now
    pruned = Pruned()
    unlinked: Counter[tuple[int, int]] = Counter()
    for deps in directories:
        files = _listing(deps)
        for name in stale_objects(files, now):
            info, path = files[name], deps / name
            try:
                # A crate compiled without incremental writes its objects again in place, under
                # the same names: a rebuild since the listing may need this one for an archive or a link.
                # Not the change time: deleting another name of the same object changes that.
                again = path.lstat()
                if (again.st_ino, again.st_mtime_ns) != (info.st_ino, info.st_mtime_ns):
                    continue
                path.unlink()
            except OSError:
                continue

            pruned.files += 1
            inode = (info.st_dev, info.st_ino)
            unlinked[inode] += 1
            if unlinked[inode] == info.st_nlink:
                pruned.bytes += info.st_blocks * 512
    return pruned


def _listing(deps: Path) -> dict[str, os.stat_result]:
    """The regular files in deps/, or none at all: a partial listing could miss the binary that needs an object."""
    files = {}
    try:
        with os.scandir(deps) as entries:
            for entry in entries:
                try:
                    info = entry.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if stat.S_ISREG(info.st_mode):
                    files[entry.name] = info
    except OSError:
        return {}
    return files


def wait_for_cargo(profile: Path, settle: float = SETTLE_SECONDS) -> None:
    """Return once no cargo is building into the profile directory.

    Started next to a cargo command, it first gives that cargo `settle` seconds to take
    its locks. A cargo that takes them later builds while the prune runs, which
    GRACE_SECONDS makes safe.
    """
    time.sleep(settle)
    for name in CARGO_LOCKS:
        try:
            with open(profile / name, "rb") as lock:
                # A shared lock waits out a build; closing the file lets go of it at once.
                fcntl.flock(lock, fcntl.LOCK_SH)
        except OSError:
            # No such lock, or a file system without locks: nothing to wait for.
            pass


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Delete the rustc objects in a cargo profile directory that nothing reads.")
    parser.add_argument("profile", type=Path, help="a cargo profile directory, such as target/debug")
    parser.add_argument("--wait", action="store_true", help="first wait until no cargo is building into it")
    args = parser.parse_args(argv)
    if args.wait:
        wait_for_cargo(args.profile)

    pruned = prune_profile(args.profile)
    if pruned.files:
        freed = pruned.bytes / 2**30
        print(f"[prune] deleted {pruned.files} stale object files in {args.profile}, {freed:.1f} GiB freed", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
