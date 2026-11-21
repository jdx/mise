#!/usr/bin/env python3
"""Test e2e scheduling and retries with a stub test executor."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class E2ERunnerTests(unittest.TestCase):
    def run_runner(
        self,
        attempts=None,
        tranche=None,
        extra_tests=(),
        args=(),
        release_skip=False,
        task_args=None,
        jobs="2",
    ):
        """Run e2e/run_all_tests, or the `mise run test:e2e` task script when
        task_args is given. Records the executed test order in self.order."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            e2e = root / "e2e"
            e2e.mkdir()
            for name in ("run_all_tests", "style.sh"):
                shutil.copy(ROOT / "e2e" / name, e2e / name)
            scripts = root / "scripts"
            scripts.mkdir()
            (scripts / "get-version.sh").write_text("echo test\n")
            if release_skip:
                (root / ".release-skip-e2e").write_text("test\n")
            task = root / "xtasks" / "test" / "e2e"
            task.parent.mkdir(parents=True)
            shutil.copyfile(ROOT / "xtasks" / "test" / "e2e", task)
            names = ("test_fail", "test_flaky", "test_pass", "test_skip_slow")
            for name in names + tuple(extra_tests):
                (e2e / name).touch()
            executor = e2e / "run_test"
            executor.write_text(
                """#!/usr/bin/env bash
set -euo pipefail
echo "$1" >> "$COUNTS.order"
count_file="$COUNTS/$1"
count=0
if [[ -f $count_file ]]; then count=$(cat "$count_file"); fi
count=$((count + 1))
echo "$count" > "$count_file"
echo "executed $1 attempt $count"
status=0
case "$1" in
  test_fail) status=1 ;;
  test_flaky) if [[ $count -eq 1 ]]; then status=1; fi ;;
  # Mimics a setup failure that dies before the test writes its own row.
  test_no_summary) exit 1 ;;
esac
echo "| $1 | attempt $count | status $status |" >> "$GITHUB_STEP_SUMMARY"
exit "$status"
"""
            )
            executor.chmod(0o755)
            counts = root / "counts"
            counts.mkdir()
            summary = root / "summary"
            env = {
                key: value
                for key, value in os.environ.items()
                if not key.startswith(("E2E_", "TEST_", "GITHUB_", "MISE_E2E_"))
            }
            env.update(
                COUNTS=str(counts),
                MISE_E2E_BIN="/unused",
                E2E_RETRY_WAIT_SECONDS="0",
                GITHUB_ACTIONS="true",
                GITHUB_STEP_SUMMARY=str(summary),
            )
            if jobs is not None:
                env["E2E_JOBS"] = jobs
            if attempts is not None:
                env["E2E_MAX_ATTEMPTS"] = str(attempts)
            if tranche is not None:
                env.update(TEST_TRANCHE_COUNT="2", TEST_TRANCHE=str(tranche))
            if task_args is None:
                command = ["bash", str(e2e / "run_all_tests"), *args]
            else:
                command = ["bash", str(task), *task_args]
            result = subprocess.run(
                command,
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            order = root / "counts.order"
            self.order = order.read_text().split() if order.exists() else []
            return (
                result,
                {p.name: int(p.read_text()) for p in counts.iterdir()},
                summary.read_text() if summary.exists() else "",
            )

    def test_retries_only_failures(self):
        result, counts, summary = self.run_runner(attempts=3)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(counts, {"test_fail": 3, "test_flaky": 2, "test_pass": 1})
        self.assertIn("executed test_flaky attempt 1", result.stdout)
        self.assertIn("executed test_flaky attempt 2", result.stdout)
        self.assertNotIn("executed test_pass", result.stdout)
        self.assertIn("E2E failures (1)::test_fail", result.stderr)
        self.assertIn("| test_flaky | attempt 2 | status 0 |", summary)
        self.assertEqual(summary.count("| test_flaky |"), 1)
        self.assertEqual(summary.count("| test_fail |"), 1)
        self.assertIn("| test_fail | attempt 3 | status 1 |", summary)

    def test_local_default_does_not_retry(self):
        result, counts, _ = self.run_runner()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(counts, {"test_fail": 1, "test_flaky": 1, "test_pass": 1})

    def test_recovered_tranche_succeeds(self):
        result, counts, _ = self.run_runner(attempts=2, tranche=1)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(counts, {"test_flaky": 2})
        self.assertNotIn("E2E failures", result.stderr)

    def test_failure_without_own_summary_row_gets_fallback(self):
        result, counts, summary = self.run_runner(
            attempts=2, extra_tests=("test_no_summary",)
        )
        self.assertEqual(result.returncode, 1)
        # Both attempts ran; neither wrote a row of its own.
        self.assertEqual(counts["test_no_summary"], 2)
        self.assertIn("| test_no_summary | - | :x: |", summary)
        self.assertEqual(summary.count("| test_no_summary |"), 1)
        self.assertIn("test_no_summary", result.stderr)

    def test_explicit_tests_run_including_slow(self):
        result, counts, _ = self.run_runner(args=("test_skip_slow", "test_pass"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(counts, {"test_skip_slow": 1, "test_pass": 1})
        self.assertIn("ran 2 tests (2 passed, 0 failed), skipped 0 tests", result.stderr)

    def test_release_skip_applies_only_to_discovery(self):
        result, counts, _ = self.run_runner(release_skip=True)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(counts, {})
        result, counts, _ = self.run_runner(release_skip=True, args=("test_pass",))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(counts, {"test_pass": 1})

    def test_task_forwards_matches_in_order_when_jobs_set(self):
        result, counts, _ = self.run_runner(
            task_args=("^test_skip_slow$", "^test_pass$"), jobs="1"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.order, ["test_skip_slow", "test_pass"])
        self.assertIn("E2E: ran 2 tests", result.stderr)

    def test_task_runs_serially_without_jobs(self):
        result, counts, _ = self.run_runner(
            task_args=("^test_skip_slow$", "^test_pass$"), jobs=None
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.order, ["test_skip_slow", "test_pass"])
        # Without E2E_JOBS the matches still go through the runner, one at a
        # time: each passing test is one line, and the run ends with a summary.
        self.assertIn("PASS: test_skip_slow\nPASS: test_pass\n", result.stderr)
        self.assertIn("E2E: ran 2 tests (2 passed, 0 failed)", result.stderr)

    def test_task_unmatched_pattern_fails_before_running(self):
        for jobs in ("2", None):
            with self.subTest(jobs=jobs):
                result, counts, _ = self.run_runner(
                    task_args=("^test_pass$", "nope"), jobs=jobs
                )
                self.assertEqual(result.returncode, 1)
                self.assertIn("No test matches nope", result.stderr)
                self.assertEqual(counts, {})

    def test_invalid_attempt_limit(self):
        for attempts in ("0", "-1", "abc", "08"):
            with self.subTest(attempts=attempts):
                result, counts, _ = self.run_runner(attempts=attempts)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(counts, {})
                self.assertIn("E2E_MAX_ATTEMPTS must be", result.stderr)


class E2ETimeLimitTests(unittest.TestCase):
    """The per-test time limit, with the real e2e/run_test running the tests."""

    def run_tests(self, tests, **settings):
        """Run e2e/run_all_tests on `tests`, a dict of test name to script."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            e2e = root / "e2e"
            e2e.mkdir()
            for name in ("run_all_tests", "run_test", "style.sh", "helpers.sh", "assert.sh"):
                shutil.copy(ROOT / "e2e" / name, e2e / name)
            for name, script in tests.items():
                (e2e / name).write_text(f"#!/usr/bin/env bash\n{script}\n")
            home = root / "home"
            home.mkdir()
            env = {
                key: value
                for key, value in os.environ.items()
                if not key.startswith(("E2E_", "TEST_", "GITHUB_", "MISE_"))
            }
            env.update(
                HOME=str(home),
                TMPDIR=str(root),
                E2E_JOBS="2",
                E2E_RETRY_WAIT_SECONDS="0",
                **settings,
            )
            result = subprocess.run(
                ["bash", str(e2e / "run_all_tests"), *tests],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=60,
            )
            return result, result.stdout + result.stderr

    @staticmethod
    def running(pid):
        state = subprocess.run(
            ["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True
        ).stdout.strip()
        return state != "" and not state.startswith("Z")

    def test_hanging_test_is_killed_with_its_output_and_not_retried(self):
        with tempfile.TemporaryDirectory() as pids:
            # A child in the test's process group, one in a group of its own
            # (as a task `mise run` starts is), and a step that never returns.
            script = f"""
echo "output before the hang"
sleep 600 &
echo "$!" >"{pids}/child"
set -m
sleep 600 &
echo "$!" >"{pids}/own-group"
set +m
sleep 600
echo "output after the hang"
"""
            result, output = self.run_tests(
                {"test_hang": script},
                E2E_TEST_TIMEOUT_SECONDS="1",
                E2E_MAX_ATTEMPTS="3",
            )
            children = [
                int((Path(pids) / name).read_text()) for name in ("child", "own-group")
            ]
        self.assertEqual(result.returncode, 1, output)
        self.assertEqual(output.count("output before the hang"), 1, output)
        self.assertNotIn("output after the hang", output)
        self.assertIn("test_hang is still running after 1s; its processes:", output)
        self.assertIn(
            "E2E test test_hang timed out: killed after exceeding its 1s time limit"
            " (E2E_TEST_TIMEOUT_SECONDS)",
            output,
        )
        self.assertNotIn("Retrying", output)
        self.assertIn("  - test_hang (timed out)", result.stderr)
        self.assertIn("ran 1 tests (0 passed, 1 failed)", result.stderr)
        for pid in children:
            self.assertFalse(self.running(pid), f"process {pid} outlived the test")

    def test_slow_tests_have_their_own_limit(self):
        result, output = self.run_tests(
            {"test_hang_slow": "sleep 600", "test_quick": "sleep 2"},
            E2E_TEST_TIMEOUT_SECONDS="30",
            E2E_SLOW_TEST_TIMEOUT_SECONDS="1",
        )
        self.assertEqual(result.returncode, 1, output)
        self.assertIn(
            "killed after exceeding its 1s time limit (E2E_SLOW_TEST_TIMEOUT_SECONDS)",
            output,
        )
        self.assertIn("PASS: test_quick", result.stderr)
        self.assertIn("ran 2 tests (1 passed, 1 failed)", result.stderr)

    def test_limit_of_zero_disables_it(self):
        result, output = self.run_tests(
            {"test_quick": "sleep 2"}, E2E_TEST_TIMEOUT_SECONDS="0"
        )
        self.assertEqual(result.returncode, 0, output)
        self.assertIn("PASS: test_quick", result.stderr)

    def test_own_exit_status_124_is_a_failure_that_is_retried(self):
        result, output = self.run_tests(
            {"test_exit_124": "exit 124"}, E2E_MAX_ATTEMPTS="2"
        )
        self.assertEqual(result.returncode, 1, output)
        self.assertEqual(output.count("exited with status code 124"), 2, output)
        self.assertNotIn("timed out", output)

    def test_invalid_time_limit(self):
        for name in ("E2E_TEST_TIMEOUT_SECONDS", "E2E_SLOW_TEST_TIMEOUT_SECONDS"):
            for value in ("-1", "abc", "08"):
                with self.subTest(name=name, value=value):
                    result, output = self.run_tests(
                        {"test_quick": "echo ran"}, **{name: value}
                    )
                    self.assertEqual(result.returncode, 1, output)
                    self.assertIn(f"{name} must be a non-negative integer", output)
                    self.assertNotIn("test_quick", output)


if __name__ == "__main__":
    unittest.main()
