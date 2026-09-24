#!/usr/bin/env bash
# SessionStart hook for Claude Code on the web. Reuses the Cloud Agent
# bootstrap in .cursor/install.sh so both runners share one setup path.
# Local sessions are left alone: the bootstrap edits /etc and runs apt-get.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
	exit 0
fi

repo_root="${CLAUDE_PROJECT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)}"

# SessionStart stdout is added to the model context; keep build noise out of it.
bash "$repo_root/.cursor/install.sh" >&2

# The bootstrap persists shims via /etc/profile.d, which non-login agent
# shells may not source. Export them for this session too.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
	cat >>"$CLAUDE_ENV_FILE" <<'ENV'
export PATH="$HOME/.local/share/mise/shims:/usr/local/bin:$PATH"
ENV
fi
