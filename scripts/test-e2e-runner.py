#!/usr/bin/env python3
"""Test e2e scheduling and retries with a stub test executor."""

import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
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
        mise_bin="stub",
        dangling_target=False,
        docker=False,
    ):
        """Run e2e/run_all_tests, or the `mise run test:e2e` task script when
        task_args is given. Records the executed test order in self.order."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            e2e = root / "e2e"
            e2e.mkdir()
            for name in ("run_all_tests", "style.sh", "helpers.sh"):
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
            # The runner refuses to start without a mise binary to test. "stub"
            # names one through MISE_E2E_BIN, "debug" puts one where a build of
            # the checkout would be, and None or any other path is passed as is.
            stub = root / "bin" / "mise"
            if mise_bin == "debug":
                stub = root / "target" / "debug" / "mise"
            if mise_bin in ("stub", "debug"):
                stub.parent.mkdir(parents=True)
                stub.write_text("#!/bin/sh\n")
                stub.chmod(0o755)
            if mise_bin == "stub":
                env["MISE_E2E_BIN"] = str(stub)
            elif mise_bin not in (None, "debug"):
                env["MISE_E2E_BIN"] = mise_bin
            if dangling_target:
                (root / "target").symlink_to(root / "gone")
            env.pop("CARGO_TARGET_DIR", None)
            env.update(
                COUNTS=str(counts),
                E2E_RETRY_WAIT_SECONDS="0",
                GITHUB_ACTIONS="true",
                GITHUB_STEP_SUMMARY=str(summary),
            )
            if docker:
                env["MISE_E2E_DOCKER"] = "1"
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

    def test_missing_binary_fails_before_running(self):
        for task_args in (None, ("^test_pass$",)):
            with self.subTest(task_args=task_args):
                result, counts, _ = self.run_runner(mise_bin=None, task_args=task_args)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(counts, {})
                self.assertIn("target/debug/mise", result.stderr)
                self.assertIn("mise run build", result.stderr)
                self.assertIn("MISE_E2E_BIN", result.stderr)
                self.assertNotIn("E2E: ran", result.stderr)

    def test_dangling_target_symlink_is_a_missing_binary(self):
        result, counts, _ = self.run_runner(mise_bin=None, dangling_target=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(counts, {})
        self.assertIn("mise run build", result.stderr)

    def test_debug_build_of_the_checkout_is_used(self):
        result, counts, _ = self.run_runner(mise_bin="debug", args=("test_pass",))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(counts, {"test_pass": 1})

    def test_selected_binary_is_the_mise_a_test_runs(self):
        """Run the real e2e/run_test with stub binaries that say who they are:
        the test calls `mise` by name and has to get the selected one."""
        for selected in ("debug", "override"):
            with self.subTest(selected=selected), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                e2e = root / "e2e"
                e2e.mkdir()
                for name in ("run_test", "style.sh", "helpers.sh"):
                    shutil.copy(ROOT / "e2e" / name, e2e / name)
                (e2e / "assert.sh").write_text("")
                (e2e / "test_which").write_text('mise >"$ROOT/seen"\n')
                (root / "test" / "data" / "plugins").mkdir(parents=True)
                # A decoy earlier on the caller's PATH must not be picked up.
                bins = {
                    "decoy": root / "decoy" / "mise",
                    "debug": root / "target" / "debug" / "mise",
                    "override": root / "elsewhere" / "mise",
                }
                for name, path in bins.items():
                    if name in ("decoy", selected):
                        path.parent.mkdir(parents=True)
                        path.write_text(f"#!/bin/sh\necho {name}\n")
                        path.chmod(0o755)
                env = {
                    key: value
                    for key, value in os.environ.items()
                    if not key.startswith(("E2E_", "TEST_", "GITHUB_", "MISE_"))
                }
                env.pop("CARGO_TARGET_DIR", None)
                env["PATH"] = f"{bins['decoy'].parent}{os.pathsep}{env['PATH']}"
                env["TMPDIR"] = str(root)
                if selected == "override":
                    env["MISE_E2E_BIN"] = str(bins["override"])
                result = subprocess.run(
                    ["bash", str(e2e / "run_test"), "test_which"],
                    cwd=root,
                    env=env,
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / "seen").read_text(), f"{selected}\n")

    def test_override_must_be_named_mise(self):
        with tempfile.TemporaryDirectory() as tmp:
            other = Path(tmp) / "mise-release"
            other.write_text("#!/bin/sh\n")
            other.chmod(0o755)
            result, counts, _ = self.run_runner(mise_bin=str(other))
            self.assertEqual(result.returncode, 1)
            # Rejected once, before any test starts or is retried.
            self.assertEqual(counts, {})
            self.assertEqual(result.stderr.count("must be named 'mise'"), 1)
            self.assertNotIn("E2E: ran", result.stderr)
            # A Docker run mounts the file as `mise`, so the name is free there.
            result, counts, _ = self.run_runner(
                mise_bin=str(other), args=("test_pass",), docker=True
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(counts, {"test_pass": 1})

    def test_override_must_be_an_executable_file(self):
        result, counts, _ = self.run_runner(mise_bin="/no/such/mise")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(counts, {})
        self.assertIn("MISE_E2E_BIN is set to", result.stderr)
        self.assertIn("/no/such/mise", result.stderr)

    def test_invalid_attempt_limit(self):
        for attempts in ("0", "-1", "abc", "08"):
            with self.subTest(attempts=attempts):
                result, counts, _ = self.run_runner(attempts=attempts)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(counts, {})
                self.assertIn("E2E_MAX_ATTEMPTS must be", result.stderr)


class E2ETimeLimitTests(unittest.TestCase):
    """The per-test time limit, with the real e2e/run_test running the tests."""

    def run_tests(self, tests, interrupt_after=None, **settings):
        """Run e2e/run_all_tests on `tests`, a dict of test name to script.
        With interrupt_after, the run gets SIGINT once that file exists."""
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
            # The runner refuses to start without a mise binary to test; these
            # tests never call it.
            stub = root / "bin" / "mise"
            stub.parent.mkdir()
            stub.write_text("#!/bin/sh\n")
            stub.chmod(0o755)
            env = {
                key: value
                for key, value in os.environ.items()
                if not key.startswith(("E2E_", "TEST_", "GITHUB_", "MISE_"))
            }
            env.pop("CARGO_TARGET_DIR", None)
            env.update(
                MISE_E2E_BIN=str(stub),
                HOME=str(home),
                TMPDIR=str(root),
                E2E_JOBS="2",
                E2E_RETRY_WAIT_SECONDS="0",
                **settings,
            )
            command = ["bash", str(e2e / "run_all_tests"), *tests]
            process = subprocess.Popen(
                command,
                cwd=root,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            try:
                if interrupt_after is not None:
                    deadline = time.monotonic() + 30
                    while not interrupt_after.exists() and time.monotonic() < deadline:
                        time.sleep(0.05)
                    process.send_signal(signal.SIGINT)
                stdout, stderr = process.communicate(timeout=60)
            finally:
                process.kill()
            result = subprocess.CompletedProcess(
                command, process.returncode, stdout, stderr
            )
            return result, stdout + stderr

    @staticmethod
    def running(pid):
        state = subprocess.run(
            ["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True
        ).stdout.strip()
        return state != "" and not state.startswith("Z")

    @staticmethod
    def hanging_test(pids):
        """A test with a child in its process group, one in a group of its own
        (as a task `mise run` starts is), and a step that never returns. It
        records the children in `pids`, then creates `ready` there."""
        return f"""
echo "output before the hang"
sleep 600 &
echo "$!" >"{pids}/child"
set -m
sleep 600 &
echo "$!" >"{pids}/own-group"
set +m
touch "{pids}/ready"
sleep 600
echo "output after the hang"
"""

    @staticmethod
    def children(pids):
        return [int((Path(pids) / name).read_text()) for name in ("child", "own-group")]

    def test_hanging_test_is_killed_with_its_output_and_not_retried(self):
        with tempfile.TemporaryDirectory() as pids:
            result, output = self.run_tests(
                {"test_hang": self.hanging_test(pids)},
                E2E_TEST_TIMEOUT_SECONDS="1",
                E2E_MAX_ATTEMPTS="3",
            )
            children = self.children(pids)
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

    def test_interrupting_the_run_stops_the_test(self):
        for limit in ("60", "0"):
            with self.subTest(limit=limit), tempfile.TemporaryDirectory() as pids:
                result, output = self.run_tests(
                    {"test_hang": self.hanging_test(pids)},
                    interrupt_after=Path(pids) / "ready",
                    E2E_TEST_TIMEOUT_SECONDS=limit,
                )
                children = self.children(pids)
                self.assertEqual(result.returncode, 130, output)
                # The run is over before the test's processes are; they get SIGTERM.
                deadline = time.monotonic() + 10
                while any(map(self.running, children)) and time.monotonic() < deadline:
                    time.sleep(0.1)
                for pid in children:
                    self.assertFalse(
                        self.running(pid), f"process {pid} outlived the run"
                    )

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


FAKE_DOCKER = r"""#!/usr/bin/env bash
# Logs each call, one line per call with every argument cut at its first line,
# and answers the few calls the e2e scripts make. A `docker run` or `docker exec`
# of a test fails when the test's name contains "fail", and the reset of a
# container exits with $DOCKER_RESET_STATUS. `docker ps` lists the containers
# named in the file $DOCKER_LIVE.
line=""
for arg in "$@"; do line+="${arg%%$'\n'*} "; done
echo "$line" >>"$DOCKER_LOG"
last="${!#}"
case "$1" in
  ps) [[ -f $DOCKER_LIVE ]] && cat "$DOCKER_LIVE" ;;
  run)
    if [[ " $* " == *" -d "* ]]; then
      echo "id-$(grep -c '^run -d ' "$DOCKER_LOG")"
    else
      [[ $last != *fail* ]]
    fi
    ;;
  exec)
    for arg in "$@"; do
      case "$arg" in
        stat) printf '1000:1000\n1000:1000\n1000:1000\n'; exit 0 ;;
        ls) printf 'core\nnull\n'; exit 0 ;;
        tmp_owner=*) exit "${DOCKER_RESET_STATUS:-0}" ;;
        /mise-src/e2e/run_test) [[ $last != *fail* ]]; exit $? ;;
      esac
    done
    ;;
esac
exit 0
"""

# Logs each call as the fake does, then runs the real docker.
LOGGING_DOCKER = r"""#!/usr/bin/env bash
line=""
for arg in "$@"; do line+="${arg%%$'\n'*} "; done
echo "$line" >>"$DOCKER_LOG"
exec "$REAL_DOCKER" "$@"
"""

# The header of a Linux executable, which is as far as the runner's check that
# the mise under test is one reads. The fake docker never runs the file.
ELF_HEADER = (
    b"\x7fELF\x02\x01\x01\x00" + bytes(8) + b"\x02\x00\x3e\x00\x01\x00\x00\x00" + bytes(40)
)

# What a test leaves behind in every place a container lets it write to, and a
# process that ignores SIGTERM.
LEAVE_THINGS = """
mkdir -p /tmp/left/dir /root/left/dir /home/left/dir
touch /tmp/left/dir/file /tmp/.left /root/left/dir/file /home/left/dir/file /dev/shm/left
chmod 700 /tmp
chown nobody /home
setsid bash -c 'trap "" TERM; exec -a e2e-left sleep 300' >/dev/null 2>&1 &
"""

# Fails if the container is not as a new one is.
EXPECT_NEW_CONTAINER = """
for path in /tmp/left /tmp/.left /root/left /home/left /dev/shm/left; do
  [[ ! -e $path ]] || { echo "$path is still there"; exit 1; }
done
[[ $(stat -c %a:%u /tmp) == 1777:$(stat -c %u /root) ]] || { echo "/tmp is $(stat -c %a:%u /tmp)"; exit 1; }
[[ $(stat -c %u /home) == "$(stat -c %u /root)" ]] || { echo "/home belongs to $(stat -c %u /home)"; exit 1; }
for cmdline in /proc/[0-9]*/cmdline; do
  arg0=
  IFS= read -r -d '' arg0 <"$cmdline" 2>/dev/null || true
  [[ $arg0 != e2e-left ]] || { echo "a process of the last test is still running"; exit 1; }
done
"""

# Kills the process that keeps the container alive, as an out-of-memory kill
# can, so the harness cannot empty the container's directories from inside.
KILL_CONTAINER = """
for cmdline in /proc/[0-9]*/cmdline; do
  arg0=
  IFS= read -r -d '' arg0 <"$cmdline" 2>/dev/null || true
  if [[ $arg0 == mise-e2e-keeper ]]; then
    pid=${cmdline#/proc/}
    kill -9 "${pid%/cmdline}"
  fi
done
sleep 30
"""

# A run_test that only says which slot it got, and fails if another running
# test holds the same one.
SLOT_RUN_TEST = """#!/usr/bin/env bash
set -euo pipefail
slot="${MISE_E2E_DOCKER_SLOT:-none}"
echo "$1 $slot" >>"$COUNTS/seen"
if [[ $slot != none ]]; then
  # Two running tests holding one slot cannot both create this directory.
  mkdir "$COUNTS/held-$slot" || { echo "$1 $slot" >>"$COUNTS/collisions"; exit 1; }
  mkdir -p "$MISE_E2E_DOCKER_SLOT_ROOT/$slot"
  echo "$MISE_E2E_DOCKER_SLOT_ROOT" >"$COUNTS/slot-root"
  sleep 0.3
  rmdir "$COUNTS/held-$slot"
fi
"""


class E2EDockerSlotTests(unittest.TestCase):
    """Container reuse in Docker mode, with a fake `docker` that logs its calls.
    The `test_real_*` tests run real containers; see `real_docker`."""

    def real_docker(self):
        """The docker, the Linux mise and the image to run real containers with. Without a
        docker, the e2e image or a Linux build of mise the test is skipped,
        unless MISE_E2E_RUNNER_DOCKER=1 says they have to be there, as CI does."""

        def missing(what):
            if os.environ.get("MISE_E2E_RUNNER_DOCKER") == "1":
                self.fail(f"MISE_E2E_RUNNER_DOCKER=1, but {what}")
            self.skipTest(what)

        docker = shutil.which("docker")
        if docker is None:
            missing("docker is not installed")
        image = os.environ.get("MISE_E2E_DOCKER_IMAGE", "ghcr.io/jdx/mise:e2e")
        inspect = subprocess.run([docker, "image", "inspect", image], capture_output=True)
        if inspect.returncode != 0:
            missing(f"the image {image} is not available")
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        binary = Path(os.environ.get("MISE_E2E_BIN", target / "debug" / "mise")).resolve()
        if not binary.is_file():
            missing(f"there is no mise build at {binary}")
        with binary.open("rb") as file:
            if file.read(4) != ELF_HEADER[:4]:
                missing(f"{binary} is not a Linux build of mise")
        return docker, binary, image

    def setup_tree(self, root, scripts, run_test=None, real=None):
        """Lay out e2e/ with the runner, a Linux binary and the fake docker, or
        with `real`, the docker and the mise that `real_docker` returned.
        Returns the environment to run it in."""
        e2e = root / "e2e"
        e2e.mkdir()
        names = ["run_all_tests", "style.sh", "helpers.sh"]
        if run_test is None:
            names += ["run_test", "assert.sh"]
        for name in names:
            shutil.copy(ROOT / "e2e" / name, e2e / name)
        if run_test is not None:
            (e2e / "run_test").write_text(run_test)
            (e2e / "run_test").chmod(0o755)
        (root / "scripts").mkdir()
        (root / "scripts" / "get-version.sh").write_text("echo test\n")
        for name, script in scripts.items():
            (e2e / name).write_text(f"#!/usr/bin/env bash\n{script}\n")
        if real is not None:
            # The container mounts the tree, and `nobody` has to read it there.
            root.chmod(0o755)
        # The runner checks that the mise under test is a Linux binary.
        binary = root / "bin" / "mise"
        binary.parent.mkdir()
        binary.write_bytes(ELF_HEADER)
        binary.chmod(0o755)
        # A binary a test can opt into with MISE_TEST_PITCHFORK=bin/pitchfork. On
        # Linux the tree is below /tmp, as a developer's own build is below /home.
        (binary.parent / "pitchfork").write_text("a pitchfork build\n")
        fake = root / "fakebin" / "docker"
        fake.parent.mkdir()
        fake.write_text(FAKE_DOCKER if real is None else LOGGING_DOCKER)
        fake.chmod(0o755)
        for name in ("counts", "hosttmp", "tmp"):
            (root / name).mkdir()
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("E2E_", "TEST_", "GITHUB_", "MISE_"))
        }
        env.pop("CARGO_TARGET_DIR", None)
        env.update(
            PATH=f"{fake.parent}{os.pathsep}{env['PATH']}",
            MISE_E2E_BIN=str(binary),
            MISE_E2E_DOCKER="1",
            MISE_E2E_HOST_TMP=str(root / "hosttmp"),
            TMPDIR=str(root / "tmp"),
            COUNTS=str(root / "counts"),
            DOCKER_LOG=str(root / "docker.log"),
            DOCKER_LIVE=str(root / "live"),
            E2E_RETRY_WAIT_SECONDS="0",
        )
        if real is not None:
            docker, mise, image = real
            env.update(
                REAL_DOCKER=docker, MISE_E2E_BIN=str(mise), MISE_E2E_DOCKER_IMAGE=image
            )
        return env

    def run_runner(self, scripts, stub=False, jobs="1", real=None, **settings):
        """Run e2e/run_all_tests with MISE_E2E_DOCKER=1 on `scripts`, a dict of
        test name to script, with the real run_test or one that only records its
        slot. Returns the result and the docker calls."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = self.setup_tree(root, scripts, SLOT_RUN_TEST if stub else None, real)
            env.update(E2E_JOBS=jobs, **settings)
            result = subprocess.run(
                ["bash", str(root / "e2e" / "run_all_tests"), *scripts],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=60 if real is None else 300,
            )
            log = root / "docker.log"
            calls = log.read_text().splitlines() if log.exists() else []
            counts = root / "counts"

            def lines(name):
                path = counts / name
                return path.read_text().split("\n")[:-1] if path.exists() else []

            self.seen = [line.split() for line in lines("seen")]
            self.collisions = lines("collisions")
            self.slot_root = next(iter(lines("slot-root")), None)
            self.slot_root_exists = bool(self.slot_root) and Path(self.slot_root).exists()
            self.leftover = sorted(p.name for p in (root / "hosttmp").iterdir())
            self.tmp_left = sorted(p.name for p in (root / "tmp").iterdir())
            return result, calls

    @staticmethod
    def starts(calls):
        return [c for c in calls if c.startswith("run -d ")]

    @staticmethod
    def exec_calls(calls):
        return [c for c in calls if "/mise-src/e2e/run_test" in c]

    def test_no_two_running_tests_share_a_slot(self):
        scripts = {f"test_{i}": "" for i in range(12)}
        result, _ = self.run_runner(scripts, stub=True, jobs="3")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.collisions, [])
        self.assertEqual(len(self.seen), 12)
        self.assertEqual({slot for _, slot in self.seen}, {"0", "1", "2"})

    def test_a_slot_is_free_again_when_its_test_is_done(self):
        scripts = {f"test_{i}": "" for i in range(3)}
        result, _ = self.run_runner(scripts, stub=True, jobs="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({slot for _, slot in self.seen}, {"0"})

    def test_opt_out_gives_no_slots(self):
        result, _ = self.run_runner(
            {"test_a": "", "test_b": ""}, stub=True, MISE_E2E_DOCKER_REUSE="0"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({slot for _, slot in self.seen}, {"none"})

    def test_no_slots_without_docker(self):
        result, _ = self.run_runner({"test_a": ""}, stub=True, MISE_E2E_DOCKER="0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.seen, [["test_a", "none"]])

    def test_invalid_reuse_setting(self):
        result, _ = self.run_runner(
            {"test_a": ""}, stub=True, MISE_E2E_DOCKER_REUSE="2"
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("MISE_E2E_DOCKER_REUSE must be 0 or 1", result.stderr)
        self.assertEqual(self.seen, [])
        self.assertEqual(self.tmp_left, [])

    def test_container_is_reused(self):
        result, calls = self.run_runner({"test_a": "", "test_b": ""})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.starts(calls)), 1, calls)
        start = self.starts(calls)[0]
        self.assertIn("--read-only", start)
        self.assertIn("--name mise-e2e-", start)
        self.assertIn("--label mise-e2e=1", start)
        self.assertIn("--init", start)
        self.assertIn("sleep infinity", start)
        self.assertEqual(len(self.exec_calls(calls)), 2)
        for call in self.exec_calls(calls):
            self.assertTrue(call.startswith("exec "), call)
        # The test's environment is the exec's, not the container's.
        self.assertNotIn(" -e ", start)
        self.assertIn(" -e MISE_E2E_INSIDE_DOCKER=1 ", self.exec_calls(calls)[0])
        # Between the two tests the container was reset.
        reset = [i for i, c in enumerate(calls) if c.startswith("exec --user root ") and "tmp_owner" in c]
        self.assertEqual(len(reset), 2, calls)

    def test_tokens_are_passed_when_set_and_only_to_the_test(self):
        result, calls = self.run_runner({"test_a": ""}, GITHUB_TOKEN="secret")
        self.assertEqual(result.returncode, 0, result.stderr)
        test_call = self.exec_calls(calls)[0]
        self.assertIn("-e GITHUB_TOKEN=secret", test_call)
        self.assertNotIn("MISE_GITHUB_TOKEN", test_call)
        self.assertEqual([c for c in calls if "secret" in c], [test_call])

    def test_failed_test_container_is_discarded(self):
        result, calls = self.run_runner({"test_a": "", "test_fail": "", "test_b": ""})
        self.assertEqual(result.returncode, 1)
        # test_a's container serves test_fail, which discards it; test_b gets a
        # new one, and the end of the run removes that one.
        self.assertEqual(len(self.starts(calls)), 2, calls)
        failing = next(i for i, c in enumerate(calls) if c.endswith("run_test test_fail "))
        second_start = [i for i, c in enumerate(calls) if c.startswith("run -d ")][1]
        removals = [i for i, c in enumerate(calls) if c.startswith("rm -f mise-e2e-")]
        self.assertTrue(any(failing < i < second_start for i in removals), calls)
        # The container that passed its test was reset, not removed.
        first_reset = next(i for i, c in enumerate(calls) if "tmp_owner" in c)
        self.assertLess(first_reset, failing)

    def test_container_that_cannot_be_reset_is_discarded(self):
        result, calls = self.run_runner(
            {"test_a": "", "test_b": ""}, DOCKER_RESET_STATUS="4"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        starts = [i for i, c in enumerate(calls) if c.startswith("run -d ")]
        self.assertEqual(len(starts), 2, calls)
        removals = [i for i, c in enumerate(calls) if c.startswith("rm -f mise-e2e-")]
        self.assertTrue(any(starts[0] < i < starts[1] for i in removals), calls)
        # The two containers do not share the directories behind /tmp.
        mounts = [
            next(arg for arg in calls[i].split() if arg.endswith(":/tmp")) for i in starts
        ]
        self.assertNotEqual(mounts[0], mounts[1])

    def test_external_binary_is_mounted_outside_the_directories_a_reset_empties(self):
        result, calls = self.run_runner(
            {"test_a": ""}, MISE_TEST_PITCHFORK="bin/pitchfork"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        mount = next(
            arg for arg in self.starts(calls)[0].split() if "/bin/pitchfork:" in arg
        )
        _, inside, mode = mount.rsplit(":", 2)
        self.assertEqual(mode, "ro")
        self.assertFalse(inside.startswith(("/tmp/", "/root/", "/home/", "/dev/")), mount)
        self.assertIn(f" -e MISE_TEST_PITCHFORK={inside} ", self.exec_calls(calls)[0])

    def test_real_container_is_clean_for_the_next_test(self):
        real = self.real_docker()
        # A mounted file cannot be removed, so one inside /tmp, /root or /home
        # would fail every reset.
        reads_binary = '[[ $(<"$MISE_TEST_PITCHFORK") == "a pitchfork build" ]] || exit 1\n'
        result, calls = self.run_runner(
            {
                "test_1_leaves_things": LEAVE_THINGS,
                "test_2_new_container": reads_binary + EXPECT_NEW_CONTAINER,
                "test_3_leaves_things": "# e2e:non-root\n"
                + reads_binary
                + "touch /tmp/left /home/left",
                "test_4_new_container": EXPECT_NEW_CONTAINER,
            },
            real=real,
            MISE_TEST_PITCHFORK="bin/pitchfork",
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(len(self.starts(calls)), 1, calls)
        self.assertEqual(len(self.exec_calls(calls)), 4, calls)
        self.assertEqual(self.leftover, [])

    def test_real_dead_container_leaves_nothing_to_the_next_one(self):
        real = self.real_docker()
        result, calls = self.run_runner(
            {
                "test_1_leaves_things": LEAVE_THINGS + KILL_CONTAINER,
                "test_2_new_container": EXPECT_NEW_CONTAINER,
            },
            real=real,
            E2E_MAX_ATTEMPTS="1",
        )
        output = result.stdout + result.stderr
        self.assertEqual(result.returncode, 1, output)
        self.assertNotIn("is still there", output)
        self.assertNotIn("test_2_new_container failed", output)
        self.assertEqual(len(self.starts(calls)), 2, calls)
        # What root left in the dead container's directories is gone too.
        self.assertEqual(self.leftover, [])

    def test_failed_attempt_is_retried_in_a_new_container(self):
        result, calls = self.run_runner({"test_fail": ""}, E2E_MAX_ATTEMPTS="2")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(self.starts(calls)), 2, calls)
        self.assertEqual(len(self.exec_calls(calls)), 2)

    def test_run_ends_with_the_containers_and_directories_gone(self):
        with tempfile.TemporaryDirectory() as tmp:
            live = Path(tmp) / "live"
            live.write_text("id-7\nid-8\n")
            result, calls = self.run_runner(
                {"test_a": ""}, stub=True, DOCKER_LIVE=str(live)
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        ps = [c for c in calls if c.startswith("ps ")]
        self.assertEqual(len(ps), 1, calls)
        self.assertIn("label=mise-e2e-run=", ps[0])
        self.assertEqual(
            sorted(c for c in calls if c.startswith("rm -f ")), ["rm -f id-7 ", "rm -f id-8 "]
        )
        self.assertTrue(self.slot_root)
        self.assertFalse(self.slot_root_exists)
        self.assertEqual(self.leftover, [])

    def test_interrupted_run_removes_containers_and_directories(self):
        for sig in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(signal=sig.name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                ready = root / "ready"
                env = self.setup_tree(
                    root,
                    {"test_hang": ""},
                    f"""#!/usr/bin/env bash
mkdir -p "$MISE_E2E_DOCKER_SLOT_ROOT/$MISE_E2E_DOCKER_SLOT"
echo "$MISE_E2E_DOCKER_SLOT_ROOT" >"{root}/slot-root"
touch "{ready}"
exec sleep 600
""",
                )
                (root / "live").write_text("id-9\n")
                env["E2E_JOBS"] = "1"
                process = subprocess.Popen(
                    ["bash", str(root / "e2e" / "run_all_tests"), "test_hang"],
                    cwd=root,
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                deadline = time.monotonic() + 30
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.05)
                process.send_signal(sig)
                process.communicate(timeout=30)
                self.assertEqual(process.returncode, 130)
                calls = (root / "docker.log").read_text().splitlines()
                self.assertIn("rm -f id-9 ", calls)
                self.assertEqual(list((root / "hosttmp").iterdir()), [])
                self.assertFalse(Path((root / "slot-root").read_text().strip()).exists())

    def test_marked_tests_keep_a_container_of_their_own(self):
        result, calls = self.run_runner(
            {"test_apt": "# e2e:writable-rootfs", "test_plain": ""}
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        own = [c for c in calls if c.startswith("run --rm ")]
        self.assertEqual(len(own), 1, calls)
        self.assertNotIn("--read-only", own[0])
        self.assertTrue(own[0].endswith("run_test test_apt "), own)
        # The other test ran in a slot container.
        self.assertEqual(len(self.starts(calls)), 1, calls)
        plain = next(c for c in calls if c.endswith("run_test test_plain "))
        self.assertTrue(plain.startswith("exec "), plain)

    def test_non_root_tests_share_the_container_as_nobody(self):
        result, calls = self.run_runner({"test_pg": "# e2e:non-root", "test_plain": ""})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.starts(calls)), 1, calls)
        self.assertNotIn("--user", self.starts(calls)[0])
        pg = next(c for c in calls if c.endswith("run_test test_pg "))
        plain = next(c for c in calls if c.endswith("run_test test_plain "))
        self.assertTrue(pg.startswith("exec --user nobody "), pg)
        self.assertNotIn("--user", plain)

    def test_opt_out_starts_a_container_per_test(self):
        result, calls = self.run_runner(
            {"test_a": "", "test_b": ""}, MISE_E2E_DOCKER_REUSE="0"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len([c for c in calls if c.startswith("run --rm ")]), 2, calls)
        self.assertEqual(self.starts(calls), [])
        self.assertEqual([c for c in calls if c.startswith("exec ")], [])

    def test_run_test_alone_starts_a_container_per_test(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = self.setup_tree(root, {"test_a": ""})
            result = subprocess.run(
                ["bash", str(root / "e2e" / "run_test"), "test_a"],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
            )
            calls = (root / "docker.log").read_text().splitlines()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 1, calls)
        self.assertTrue(calls[0].startswith("run --rm --read-only "), calls)


if __name__ == "__main__":
    unittest.main()
