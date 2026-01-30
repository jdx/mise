#!/usr/bin/env python3
#MISE description="Retry failed test-tools with grace period for recent upstream releases"
#USAGE flag "--grace-period" help="Ignore failures from tools whose upstream released <7 days ago"
#USAGE arg "<tools>..." help="Failed tools to retry"
"""Retries failed test-tool runs. With --grace-period, tools backed by
GitHub/aqua whose latest upstream release is less than 7 days old have
their failures treated as warnings instead of errors."""

import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone, timedelta
from pathlib import Path

GRACE_PERIOD = timedelta(days=7)


def get_repo(tool: str) -> str | None:
    """Extract the GitHub owner/repo from a tool's backend via mise."""
    try:
        result = subprocess.run(
            ["mise", "tool", "--json", tool],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        backend = data.get("backend", "")
        # backend looks like "github:owner/repo[bin=x]" or "aqua:owner/repo/subpath"
        for prefix in ("github:", "aqua:"):
            if backend.startswith(prefix):
                repo = backend[len(prefix):]
                # strip any trailing [options]
                repo = repo.split("[")[0]
                # aqua backends can have subpaths (e.g. kubernetes/kubernetes/kubectl)
                # extract just owner/repo for the GitHub API
                parts = repo.split("/")
                if len(parts) >= 2:
                    return f"{parts[0]}/{parts[1]}"
                return None
        return None
    except (subprocess.TimeoutExpired, json.JSONDecodeError, KeyError):
        return None


def get_latest_release_date(repo: str) -> datetime | None:
    """Get the published_at date of the latest GitHub release."""
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/releases/latest", "--jq", ".published_at"],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0 or not result.stdout.strip():
            return None
        return datetime.fromisoformat(result.stdout.strip().replace("Z", "+00:00"))
    except (subprocess.TimeoutExpired, subprocess.SubprocessError, ValueError):
        return None


def get_failed_tools_from_summary() -> list[str]:
    """Parse failed tools from GITHUB_STEP_SUMMARY."""
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path or not Path(summary_path).exists():
        return []
    content = Path(summary_path).read_text()
    failed = []
    for line in content.splitlines():
        if "Failed Tools" in line:
            cleaned = re.sub(r"\*\*Failed Tools\*\*:\s*", "", line).strip()
            failed = [t.strip() for t in cleaned.split(",") if t.strip()]
    return failed


def retry_tools(tools: list[str]) -> list[str]:
    """Retry failed tools and return any that still fail."""
    result = subprocess.run(["mise", "test-tool"] + tools)
    if result.returncode == 0:
        return []
    failed = get_failed_tools_from_summary()
    return failed if failed else tools


def check_grace_period(tools: list[str]) -> list[str]:
    """Return tools that are NOT within the grace period."""
    hard_failures = []
    now = datetime.now(timezone.utc)

    for tool in tools:
        repo = get_repo(tool)
        if not repo:
            print(f"::error::{tool}: no github/aqua backend found")
            hard_failures.append(tool)
            continue

        published = get_latest_release_date(repo)
        if not published:
            print(f"::error::{tool}: could not fetch latest release for {repo}")
            hard_failures.append(tool)
            continue

        age = now - published
        if age < GRACE_PERIOD:
            print(f"::warning::Ignoring {tool} failure — latest release of {repo} "
                  f"({published.isoformat()}) is {age.days}d old (< 7d grace period)")
        else:
            print(f"::error::{tool}: latest release of {repo} is {age.days}d old")
            hard_failures.append(tool)

    return hard_failures


def main():
    grace_period = "--grace-period" in sys.argv
    tools = [a for a in sys.argv[1:] if not a.startswith("-")]

    if not tools:
        print("Usage: test-tool-retry [--grace-period] <tool1> [tool2] ...")
        sys.exit(1)

    still_failing = retry_tools(tools)
    if not still_failing:
        print("All tools passed on retry.")
        sys.exit(0)

    if not grace_period:
        print(f"Failed tools: {', '.join(still_failing)}")
        sys.exit(1)

    hard_failures = check_grace_period(still_failing)
    if hard_failures:
        print(f"\nHard failures: {', '.join(hard_failures)}")
        sys.exit(1)

    print("\nAll failures are within the 7-day grace period for new releases.")


if __name__ == "__main__":
    main()
