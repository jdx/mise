#!/usr/bin/env python3
#MISE description="Check if test-tool failures are from recent upstream releases (<7 days)"
"""For each failed tool, looks up its registry entry to find the GitHub repo,
then checks if the latest release was published within the last 7 days.
Tools with recent releases are assumed to have transient asset issues
and their failures are ignored."""

import re
import subprocess
import sys
from datetime import datetime, timezone, timedelta
from pathlib import Path

GRACE_PERIOD = timedelta(days=7)
REGISTRY_DIR = Path(__file__).resolve().parent.parent / "registry"


def get_repo(tool: str) -> str | None:
    toml_path = REGISTRY_DIR / f"{tool}.toml"
    if not toml_path.exists():
        return None
    content = toml_path.read_text()
    m = re.search(r'(?:github|aqua):([^"\]\s]+)', content)
    if not m:
        return None
    return re.sub(r"\[.*", "", m.group(1))


def get_latest_release_date(repo: str) -> datetime | None:
    try:
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/releases/latest", "--jq", ".published_at"],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0 or not result.stdout.strip():
            return None
        return datetime.fromisoformat(result.stdout.strip().replace("Z", "+00:00"))
    except (subprocess.TimeoutExpired, Exception):
        return None


def main():
    tools = sys.argv[1:]
    if not tools:
        print("Usage: check-release-failures <tool1> [tool2] ...")
        sys.exit(1)

    hard_failures = []
    now = datetime.now(timezone.utc)

    for tool in tools:
        repo = get_repo(tool)
        if not repo:
            print(f"::error::{tool}: no github/aqua backend found in registry")
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

    if hard_failures:
        print(f"\nHard failures: {', '.join(hard_failures)}")
        sys.exit(1)

    print("\nAll failures are within the 7-day grace period for new releases.")


if __name__ == "__main__":
    main()
