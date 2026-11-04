"""guard.py 的判定表：python3 scripts/verify/test_guard.py"""

import unittest

from guard import inspect

FULL = [
    "cargo test --workspace --lib",
    "CARGO_TARGET_DIR=target/iso cargo clippy --workspace --all-targets -- -D warnings",
    "nice -n 10 cargo clippy --all --all-targets",
    "cd web && npx playwright test",
    "cd web; ./node_modules/.bin/playwright test --grep-invert @perf --output /tmp/x 2>&1 | tail -5",
    "pnpm -C web playwright",
    "npm run playwright",
    "cd web && ./node_modules/.bin/vitest run",
    "pnpm -C web test:run",
    "npm test",
    "pnpm -C web lint",
    "npm run lint",
    "cd web && npx eslint . --max-warnings 0",
    "make ci",
    "make -C /repo lint",
    "make test",
    "scripts/verify/verify.py --all",
    "python3 scripts/verify/verify.py --all --fresh",
    "bash -lc 'cd web && npx playwright test'",
    "for i in 1 2; do cargo test --workspace; done",
]
HEAVY = [
    "cargo test -p molesignal --lib",
    "CARGO_TARGET_DIR=$PWD/target/iso-x nice -n 10 cargo clippy -p domain@0.1.0 --all-targets",
    "cargo check -p molesignal 2>&1 | tail -3",
    "cargo build -p probe-agent",
    "cd web && npx playwright test tests/foo.spec.ts --workers=2",
    "cd web && ./node_modules/.bin/playwright test -g 'a|b'",
    "make build-debug",
    "cd web && ./node_modules/.bin/tsc -b --noEmit",
    "pnpm -C web typecheck",
    "cd web && ./node_modules/.bin/vitest run src/a.test.ts",
    "cd web && npx vitest related src/a.ts --run",
    "pnpm -C web test:run src/a.test.ts",
    "cd web && npx eslint src/a.tsx src/b.tsx --max-warnings 0",
]
ALLOWED = [
    "scripts/verify/verify.py web/src/a.tsx --specs foo.spec.ts",
    "scripts/verify/verify.py --run -- cargo test -p signals --lib",
    "python3 /abs/scripts/verify/verify.py --status",
    "cargo fmt --all",
    "cargo +nightly fmt --all -- --check",
    "cargo metadata --no-deps",
    "cargo test --help",
    "cargo tree -p molesignal",
    "cargo run -p molesignal",
    "make run",
    "make fmt",
    "make web-build",
    "cd web && ./node_modules/.bin/tsc --version",
    "pnpm -C web lint:no-black",
    "cd web && npx playwright test --list",
    "git commit -m 'run cargo test --workspace; playwright test'",
    "grep -rn 'cargo test --workspace' docs",
    "echo cargo test --workspace",
    "python3 - <<'EOF'\nprint('cargo test --workspace')\nEOF",
    "cat > /tmp/run.sh <<EOF\ncargo test --workspace\nEOF\n",
    "ls -la target",
]


class GuardTable(unittest.TestCase):
    def test_full_commands_are_refused_even_through_run(self):
        for command in FULL:
            with self.subTest(command=command):
                findings = inspect(command)
                self.assertTrue(findings and any(f.full for f in findings), findings)

    def test_heavy_commands_go_through_the_queue(self):
        for command in HEAVY:
            with self.subTest(command=command):
                findings = inspect(command)
                self.assertTrue(findings, command)
                self.assertFalse(any(f.full for f in findings), findings)

    def test_everything_else_passes(self):
        for command in ALLOWED:
            with self.subTest(command=command):
                self.assertEqual(inspect(command), [])


if __name__ == "__main__":
    unittest.main()
