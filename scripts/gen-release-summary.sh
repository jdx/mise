#!/usr/bin/env bash
set -euo pipefail

# Generate a prose summary of release notes using Claude Code
# Usage: ./scripts/gen-release-summary.sh [version] [prev_version]

version="${1:-}"
prev_version="${2:-}"

if [[ -z $version ]]; then
	echo "Usage: $0 <version> [prev_version]" >&2
	exit 1
fi

# Get the git-cliff changelog for context
changelog=$(git cliff --unreleased --strip all 2>/dev/null || echo "")

if [[ -z $changelog ]]; then
	echo "No unreleased changes found" >&2
	exit 0
fi

# Use Claude Code to generate summary with full repo context
# Sandboxed: only read-only tools allowed (no Bash, Edit, Write)
claude -p \
	--model claude-opus-4-20250514 \
	--output-format text \
	--allowedTools "Read,Grep,Glob" \
	<<EOF
You are generating release notes for mise version ${version}${prev_version:+ (previous version: ${prev_version})}.

Here is the structured changelog from git-cliff:
${changelog}

Write a 2-3 paragraph prose summary of this release. Focus on:
- The most impactful changes for users
- Key new features and why they matter
- Important bug fixes
- Any breaking changes or migration notes

Be concise and write in flowing prose (no bullet points). The tone should be informative and professional, similar to a blog post announcement.

Output ONLY the prose summary, no preamble or explanation.
EOF
