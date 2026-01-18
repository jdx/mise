#!/usr/bin/env bash
set -euo pipefail

# Generate editorialized release notes using Claude Code
# Usage: ./scripts/gen-release-summary.sh <version> [prev_version]

version="${1:-}"
prev_version="${2:-}"

if [[ -z $version ]]; then
	echo "Usage: $0 <version> [prev_version]" >&2
	exit 1
fi

# Get the git-cliff changelog for context
changelog=$(git cliff --unreleased --strip all 2>/dev/null || echo "")

if [[ -z $changelog ]]; then
	echo "Error: No unreleased changes found" >&2
	exit 1
fi

# Use Claude Code to editorialize the release notes
# Sandboxed: only read-only tools allowed (no Bash, Edit, Write)
output=$(
	claude -p \
		--model claude-opus-4-20250514 \
		--output-format text \
		--allowedTools "Read,Grep,Glob" \
		<<EOF
You are writing release notes for mise version ${version}${prev_version:+ (previous version: ${prev_version})}.

Here is the raw changelog from git-cliff:
${changelog}

Rewrite this into user-friendly release notes. The format should be:

1. Start with 1-2 paragraphs summarizing the most important changes
2. Then organize into sections using ### headers (e.g., "### Highlights", "### Bug Fixes")
3. Write in clear, user-focused language (not developer commit messages)
4. Explain WHY changes matter to users, not just what changed
5. Group related changes together logically
6. Skip minor/internal changes that don't affect users
7. Include contributor attribution where appropriate (@username)

IMPORTANT: Use only ### for section headers. NEVER use "## [" as this pattern is reserved for version headers and will corrupt changelog processing.

Keep the tone professional but approachable. Focus on what users care about.

Output ONLY the editorialized release notes, no preamble.
EOF
)

# Validate output doesn't contain patterns that would corrupt changelog processing
if echo "$output" | grep -qE '^## \['; then
	echo "Error: LLM output contains '## [' pattern which would corrupt changelog processing" >&2
	exit 1
fi

echo "$output"
