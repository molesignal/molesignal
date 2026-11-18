"""Which object files prune.py deletes, and when: python3 scripts/verify/test_prune.py"""

import fcntl
import io
import os
import tempfile
import threading
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from stat import S_IFREG
from types import SimpleNamespace
from unittest import mock

import prune
from prune import GRACE_SECONDS, Pruned, prune_objects, prune_profile, stale_objects, wait_for_cargo

HOUR = 60 * 60
NOW = 1_800_000_000.0
# A test binary and an rlib, named the way cargo names them in deps/.
TEST = "molesignal-0123456789abcdef"
LIB = "kernel-fedcba9876543210"


def info(changed: float, modified: float | None = None) -> SimpleNamespace:
    return SimpleNamespace(st_ctime=changed, st_mtime=changed if modified is None else modified, st_mode=S_IFREG)


def compiled(unit: str, session: str, at: float, count: int = 2) -> dict:
    """The objects one incremental compile leaves behind; each compile names them anew."""
    return {f"{unit}.cgu{n}.{session}.rcgu.o": info(at) for n in range(count)}


class Rule(unittest.TestCase):
    def test_a_test_binary_keeps_the_objects_it_was_linked_from(self):
        linked = NOW - 3 * HOUR
        files = {
            TEST: info(linked),
            f"{TEST}.d": info(linked),
            **compiled(TEST, "s1", NOW - 30 * HOUR),
            **compiled(TEST, "s2", NOW - 9 * HOUR),
            **compiled(TEST, "s3", linked - 60),
        }
        self.assertEqual(stale_objects(files, NOW), sorted([*compiled(TEST, "s1", 0), *compiled(TEST, "s2", 0)]))

    def test_a_slow_compile_keeps_all_its_objects(self):
        # Under load one molesignal compile wrote its first object 19 minutes before linking.
        linked = NOW - 5 * HOUR
        files = {TEST: info(linked), **compiled(TEST, "s1", linked - 40 * 60)}
        self.assertEqual(stale_objects(files, NOW), [])

    def test_an_object_reused_from_incremental_counts_from_its_new_name(self):
        # Hard-linked from incremental/: the old modification time stays, the change time is new.
        linked = NOW - 5 * HOUR
        files = {TEST: info(linked), f"{TEST}.cgu0.s2.rcgu.o": info(linked - 60, modified=NOW - 72 * HOUR)}
        self.assertEqual(stale_objects(files, NOW), [])

    def test_an_rlib_keeps_no_loose_objects(self):
        # Binaries read the rlib's members, never the loose copies.
        built = NOW - 2 * HOUR
        files = {
            f"lib{LIB}.rlib": info(built),
            f"lib{LIB}.rmeta": info(built),
            f"{LIB}.d": info(built),
            **compiled(LIB, "s1", built - 60),
        }
        self.assertEqual(stale_objects(files, NOW), sorted(compiled(LIB, "s1", 0)))

    def test_recent_objects_stay_whatever_their_unit(self):
        # A compile that has not archived or linked them yet may still be running.
        recent = NOW - GRACE_SECONDS + 60
        files = {
            f"lib{LIB}.rlib": info(NOW - 3 * HOUR),
            **compiled(LIB, "s2", recent),
            **compiled(TEST, "s1", recent),
        }
        self.assertEqual(stale_objects(files, NOW), [])

    def test_objects_whose_unit_has_no_output_left_go(self):
        files = {f"{TEST}.d": info(NOW - 4 * HOUR), **compiled(TEST, "s1", NOW - 4 * HOUR)}
        self.assertEqual(stale_objects(files, NOW), sorted(compiled(TEST, "s1", 0)))

    def test_a_dylib_is_linked_like_an_executable(self):
        macro = "permission_macro-00112233445566ff"
        linked = NOW - 3 * HOUR
        files = {f"lib{macro}.dylib": info(linked), **compiled(macro, "s1", linked - 60)}
        self.assertEqual(stale_objects(files, NOW), [])

    def test_units_are_told_apart_by_hash_not_by_name(self):
        # liblibc-<hash>.rlib belongs to libc-<hash>.*.rcgu.o; a test binary of the same crate is another unit.
        libc_rlib, libc_test = "libc-1111111111111111", "libc-2222222222222222"
        old = NOW - 4 * HOUR
        files = {
            f"lib{libc_rlib}.rlib": info(old),
            **compiled(libc_rlib, "s1", old),
            libc_test: info(old),
            **compiled(libc_test, "s1", old),
        }
        self.assertEqual(stale_objects(files, NOW), sorted(compiled(libc_rlib, "s1", 0)))

    def test_only_rustc_objects_are_ever_chosen(self):
        old = NOW - 100 * HOUR
        files = {
            TEST: info(old),
            f"{TEST}.d": info(old),
            f"lib{LIB}.rlib": info(old),
            f"lib{LIB}.rmeta": info(old),
            f"{LIB}.long-type-1234.txt": info(old),
            f"{LIB}.helper.o": info(old),
            "README.o": info(old),
            "stray.rcgu.o": info(old),
        }
        self.assertEqual(stale_objects(files, NOW), [])


class Prune(unittest.TestCase):
    """prune_objects on a real directory: file times are real, so `now` and the binaries' times are moved instead."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.target = Path(self.temporary.name)
        self.deps = self.target / "debug" / "deps"
        self.deps.mkdir(parents=True)
        self.created = time.time()

    def tearDown(self):
        self.temporary.cleanup()

    def write(self, name: str, size: int = 8192, modified: float | None = None) -> Path:
        path = self.deps / name
        path.write_bytes(b"\0" * size)
        if modified is not None:
            os.utime(path, (modified, modified))
        return path

    def test_deletes_only_what_nothing_reads(self):
        # TEST was relinked two hours after its objects were written; LIB keeps none.
        self.write(TEST, modified=self.created + 2 * HOUR)
        old = [self.write(name) for name in compiled(TEST, "s1", 0)]
        current_test = "molesignal-00000000000000aa"
        self.write(current_test, modified=self.created + 30 * 60)
        kept = [self.write(name) for name in compiled(current_test, "s2", 0)]
        rlib = [self.write(f"lib{LIB}.rlib"), self.write(f"lib{LIB}.rmeta"), self.write(f"{LIB}.d")]
        loose = [self.write(name) for name in compiled(LIB, "s1", 0)]

        pruned = prune_objects(self.target, now=self.created + 3 * HOUR)

        self.assertEqual(pruned.files, len(old) + len(loose))
        self.assertTrue(all(not path.exists() for path in [*old, *loose]))
        self.assertTrue(all(path.exists() for path in [*kept, *rlib, self.deps / TEST, self.deps / current_test]))

    def test_counts_space_only_when_the_last_link_goes(self):
        shared = self.write(f"{LIB}.cgu0.s1.rcgu.o")
        incremental = self.target / "debug" / "incremental"
        incremental.mkdir()
        os.link(shared, incremental / "cgu0.o")
        alone = self.write(f"{LIB}.cgu1.s1.rcgu.o")
        blocks = alone.stat().st_blocks * 512

        pruned = prune_objects(self.target, now=self.created + 2 * HOUR)

        self.assertEqual((pruned.files, pruned.bytes), (2, blocks))
        self.assertTrue((incremental / "cgu0.o").exists())

    def test_deletes_every_name_of_an_object_reused_by_later_compiles(self):
        # Incremental compilation links a reused object into deps/ under each compile's name.
        # Deleting one name changes the change time of the others, which must go all the same.
        names = [self.write(f"{LIB}.cgu0.s1.rcgu.o")]
        for session in ("s2", "s3"):
            names.append(self.deps / f"{LIB}.cgu0.{session}.rcgu.o")
            os.link(names[0], names[-1])
        blocks = names[0].stat().st_blocks * 512

        pruned = prune_objects(self.target, now=self.created + 2 * HOUR)

        self.assertEqual((pruned.files, pruned.bytes), (3, blocks))
        self.assertFalse(any(path.exists() for path in names))

    def test_leaves_a_running_compile_alone(self):
        objects = [self.write(name) for name in compiled(LIB, "s1", 0)]
        pruned = prune_objects(self.target, now=self.created + 10 * 60)
        self.assertEqual(pruned.files, 0)
        self.assertTrue(all(path.exists() for path in objects))

    def test_covers_every_profile_and_tolerates_a_missing_target(self):
        release = self.target / "release" / "deps"
        release.mkdir(parents=True)
        (release / f"{LIB}.cgu0.s1.rcgu.o").write_bytes(b"\0")
        self.assertEqual(prune_objects(self.target, now=self.created + 2 * HOUR).files, 1)
        self.assertEqual(prune_objects(self.target / "missing").files, 0)

    def test_a_profile_alone(self):
        release = self.target / "release" / "deps"
        release.mkdir(parents=True)
        other = release / f"{LIB}.cgu0.s1.rcgu.o"
        other.write_bytes(b"\0")
        mine = self.write(f"{LIB}.cgu0.s1.rcgu.o")

        pruned = prune_profile(self.target / "debug", now=self.created + 2 * HOUR)

        self.assertEqual(pruned.files, 1)
        self.assertFalse(mine.exists())
        self.assertTrue(other.exists())
        self.assertEqual(prune_profile(self.target / "missing").files, 0)

    def test_spares_an_object_written_again_after_the_listing(self):
        # Without incremental compilation rustc rewrites an object in place: same name, same inode, new change time.
        def in_place(path: Path) -> None:
            time.sleep(0.01)
            path.write_bytes(b"\1")

        def replaced(path: Path) -> None:
            fresh = path.with_name("fresh.tmp")
            fresh.write_bytes(b"\1")
            os.replace(fresh, path)

        listing = prune._listing
        for rebuild in (in_place, replaced):
            with self.subTest(rebuild.__name__):
                path = self.write(f"{LIB}.cgu0.s1.rcgu.o")

                def listing_then_rebuild(deps: Path) -> dict:
                    files = listing(deps)
                    rebuild(path)
                    return files

                with mock.patch.object(prune, "_listing", listing_then_rebuild):
                    pruned = prune_objects(self.target, now=self.created + 2 * HOUR)

                self.assertEqual(pruned.files, 0)
                self.assertEqual(path.read_bytes(), b"\1")


class WaitForCargo(unittest.TestCase):
    """The locks are real: flock locks taken through two open() calls conflict even within one process."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.profile = Path(self.temporary.name)
        for name in prune.CARGO_LOCKS:
            (self.profile / name).touch()

    def tearDown(self):
        self.temporary.cleanup()

    def test_waits_until_cargo_lets_go_of_its_build_lock(self):
        waiter = threading.Thread(target=wait_for_cargo, args=(self.profile,), kwargs={"settle": 0}, daemon=True)
        with open(self.profile / ".cargo-build-lock", "rb") as cargo:
            fcntl.flock(cargo, fcntl.LOCK_EX)
            waiter.start()
            waiter.join(0.5)
            self.assertTrue(waiter.is_alive())

        waiter.join(5)
        self.assertFalse(waiter.is_alive())

    def test_does_not_wait_when_no_cargo_builds(self):
        started = time.monotonic()
        wait_for_cargo(self.profile, settle=0)
        wait_for_cargo(self.profile / "missing", settle=0)
        self.assertLess(time.monotonic() - started, 1)


class Command(unittest.TestCase):
    def run_main(self, argv: list[str], pruned: Pruned) -> tuple[str, list]:
        calls, output = [], io.StringIO()

        def waiting(profile: Path) -> None:
            calls.append(("wait", profile))

        def pruning(profile: Path) -> Pruned:
            calls.append(("prune", profile))
            return pruned

        with mock.patch.object(prune, "wait_for_cargo", waiting), mock.patch.object(prune, "prune_profile", pruning):
            with redirect_stdout(output):
                self.assertEqual(prune.main(argv), 0)
        return output.getvalue(), calls

    def test_prunes_the_profile_and_says_what_went(self):
        output, calls = self.run_main(["target/debug"], Pruned(files=3, bytes=3 * 2**30))
        self.assertEqual(calls, [("prune", Path("target/debug"))])
        self.assertEqual(output, "[prune] deleted 3 stale object files in target/debug, 3.0 GiB freed\n")

    def test_waits_for_cargo_first_and_stays_quiet_when_nothing_goes(self):
        output, calls = self.run_main(["target/debug", "--wait"], Pruned())
        self.assertEqual(calls, [("wait", Path("target/debug")), ("prune", Path("target/debug"))])
        self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()
