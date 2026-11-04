"""测试失败的归属判断：python3 scripts/verify/test_steps.py"""

import unittest
from types import SimpleNamespace

from steps import foreign_test_failures

WORKER_SET = """
---- coverage::tests::every_worker stdout ----

thread 'coverage::tests::every_worker' (1) panicked at crates/support/signals/src/coverage.rs:292:9:
assertion `left == right` failed
  left: {"src/bootstrap/workers/acme.rs", "src/bootstrap/workers/old.rs"}
 right: {"src/bootstrap/workers/acme.rs", "src/bootstrap/workers/%s"}
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    coverage::tests::every_worker

test result: FAILED. 63 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.97s

error: test failed, to rerun pass `-p signals --lib`
error: 1 target failed:
    `-p signals --lib`
"""
BOUNDARY = """
---- coverage::tests::boundary stdout ----

thread 'coverage::tests::boundary' (1) panicked at crates/support/signals/src/coverage.rs:324:9:
production SQL transactions bypass the traced facade: {"crates/engines/postgres/src/alert_routing.rs"}


failures:
    coverage::tests::boundary

test result: FAILED. 63 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.97s

error: 1 target failed:
    `-p signals --lib`
"""
PLAIN = """
---- tests::adds stdout ----

thread 'tests::adds' (1) panicked at crates/core/a/src/lib.rs:9:9:
assertion `left == right` failed
  left: 1
 right: 2


failures:
    tests::adds

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 1 target failed:
    `-p a --lib`
"""


def context(own=(), dirty=()):
    return SimpleNamespace(own=set(own), dirty=set(own) | set(dirty))


class Attribution(unittest.TestCase):
    def test_set_difference_points_at_another_sessions_worker(self):
        ctx = context(dirty=["bin/molesignal/src/bootstrap/workers/new.rs", "bin/molesignal/src/bootstrap/workers/old.rs"])
        output = WORKER_SET.replace('workers/%s"', 'workers/new.rs"')
        self.assertEqual(foreign_test_failures(ctx, output), ["coverage::tests::every_worker"])

    def test_my_own_worker_is_my_failure(self):
        ctx = context(
            own=["bin/molesignal/src/bootstrap/workers/new.rs"],
            dirty=["bin/molesignal/src/bootstrap/workers/old.rs"],
        )
        output = WORKER_SET.replace('workers/%s"', 'workers/new.rs"')
        self.assertEqual(foreign_test_failures(ctx, output), [])

    def test_committed_file_is_not_someone_elses_half_work(self):
        self.assertEqual(foreign_test_failures(context(), BOUNDARY), [])

    def test_uncommitted_file_of_another_session_is_ignored(self):
        ctx = context(dirty=["crates/engines/postgres/src/alert_routing.rs"])
        self.assertEqual(foreign_test_failures(ctx, BOUNDARY), ["coverage::tests::boundary"])

    def test_a_failure_naming_no_file_stays_mine(self):
        ctx = context(dirty=["crates/core/a/src/lib.rs"])
        self.assertEqual(foreign_test_failures(ctx, PLAIN), [])

    def test_a_crashed_test_binary_is_never_ignored(self):
        ctx = context(dirty=["crates/engines/postgres/src/alert_routing.rs"])
        crashed = BOUNDARY.replace("error: 1 target failed:", "error: 2 targets failed:")
        self.assertEqual(foreign_test_failures(ctx, crashed), [])


if __name__ == "__main__":
    unittest.main()
