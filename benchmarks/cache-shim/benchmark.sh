#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
mise_bin=${MISE_BIN:-"$repo_root/target/release/mise"}
runs=${CACHE_SHIM_BENCH_RUNS:-200}
budget_ms=${CACHE_SHIM_BENCH_BUDGET_MS:-2}

if [[ ! $runs =~ ^[1-9][0-9]*$ ]] || ! [[ $budget_ms =~ ^[0-9]+([.][0-9]+)?$ ]]; then
	echo "run count must be positive and budget must be a non-negative number" >&2
	exit 2
fi
if [[ ! -x $mise_bin ]]; then
	echo "mise benchmark binary is not executable: $mise_bin" >&2
	exit 2
fi

fixture=$(mktemp -d "${TMPDIR:-/tmp}/mise-cache-shim-bench.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
shim="$fixture/mise-cache-rustc"
ln "$mise_bin" "$shim" 2>/dev/null || cp "$mise_bin" "$shim"

python3 - "$shim" "$runs" "$budget_ms" <<'PY'
import statistics
import subprocess
import sys
import time

shim, runs, budget_ms = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])

def invoke(command):
    started = time.perf_counter_ns()
    subprocess.run(command, check=True)
    return (time.perf_counter_ns() - started) / 1_000_000

for _ in range(10):
    invoke(["true"])
    invoke([shim, "true"])
direct = [invoke(["true"]) for _ in range(runs)]
wrapped = [invoke([shim, "true"]) for _ in range(runs)]
direct_median = statistics.median(direct)
wrapped_median = statistics.median(wrapped)
overhead = wrapped_median - direct_median
p95 = sorted(wrapped)[max(0, int(len(wrapped) * 0.95) - 1)]
print(
    f"cache shim warm exec: direct={direct_median:.3f}ms "
    f"wrapped={wrapped_median:.3f}ms overhead={overhead:.3f}ms "
    f"wrapped-p95={p95:.3f}ms runs={runs} budget={budget_ms:.3f}ms"
)
if overhead > budget_ms:
    raise SystemExit(
        f"cache shim overhead {overhead:.3f}ms exceeds {budget_ms:.3f}ms budget"
    )
PY
