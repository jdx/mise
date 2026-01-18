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

# Build prompt safely using printf to avoid command substitution on backticks in changelog
# Using printf %s ensures no interpretation of backslashes or special characters
prompt=$(
	printf '%s\n' "You are writing release notes for mise version ${version}${prev_version:+ (previous version: ${prev_version})}."
	printf '\n'
	printf '%s\n' "mise is a polyglot runtime manager (like asdf, nvm, pyenv, etc), environment manager, and task runner."
	printf '\n'
	printf '%s\n' "Here is the raw changelog from git-cliff:"
	printf '%s\n' "$changelog"
	printf '\n'
	cat <<'INSTRUCTIONS'
Rewrite this into user-friendly release notes. The format should be:

1. Start with 1-2 paragraphs summarizing the most important changes
2. Then organize into sections using ### headers (e.g., "### Highlights", "### Bug Fixes")
3. Write in clear, user-focused language (not developer commit messages)
4. Explain WHY changes matter to users, not just what changed
5. Group related changes together logically
6. Skip minor/internal changes that don't affect users
7. Include contributor attribution where appropriate (@username)
8. Include links to PRs (e.g., [#1234](https://github.com/jdx/mise/pull/1234)) for significant changes
9. Where applicable, link to relevant documentation at https://mise.jdx.dev/

IMPORTANT: Use only ### for section headers. NEVER use "## [" as this pattern is reserved for version headers and will corrupt changelog processing.

Keep the tone professional but approachable. Focus on what users care about.

Output ONLY the editorialized release notes, no preamble.
INSTRUCTIONS
)

# Use Claude Code to editorialize the release notes
# Sandboxed: only read-only tools allowed (no Bash, Edit, Write)
output=$(
	printf '%s' "$prompt" | claude -p \
		--model claude-opus-4-20250514 \
		--output-format text \
		--allowedTools "Read,Grep,Glob"
)

# Validate output doesn't contain patterns that would corrupt changelog processing
if echo "$output" | grep -qE '^## \['; then
	echo "Error: LLM output contains '## [' pattern which would corrupt changelog processing" >&2
	exit 1
fi

echo "$output"
