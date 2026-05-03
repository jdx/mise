#!/usr/bin/env bash
# Generate aqua-registry changelog section
# Usage: gen-aqua-changelog.sh <old_tag> <new_tag> <heading_level>
#
# Lists packages added or modified in the upstream aqua-registry between
# OLD_TAG and NEW_TAG by diffing the merged registry.yaml at each tag.
set -euo pipefail

OLD_TAG="${1:-}"
NEW_TAG="${2:-}"
HEADING_LEVEL="${3:-###}" # Default to ### for CHANGELOG.md sections
REPO="aquaproj/aqua-registry"
NEW_REGISTRY="vendor/aqua-registry/registry.yml"

if [[ -z $OLD_TAG ]] || [[ -z $NEW_TAG ]] || [[ $OLD_TAG == "$NEW_TAG" ]]; then
	exit 0
fi

if [[ ! -f $NEW_REGISTRY ]]; then
	echo "Expected $NEW_REGISTRY to exist" >&2
	exit 0
fi

OLD_REGISTRY="$(mktemp)"
trap 'rm -f "$OLD_REGISTRY"' EXIT

if ! curl -fsSL "https://raw.githubusercontent.com/$REPO/$OLD_TAG/registry.yaml" -o "$OLD_REGISTRY"; then
	echo "Failed to fetch aqua-registry $OLD_TAG/registry.yaml" >&2
	exit 0
fi

python3 - "$OLD_REGISTRY" "$NEW_REGISTRY" "$HEADING_LEVEL" <<'PYEOF'
import re
import sys

old_path, new_path, heading = sys.argv[1:4]


def strip_quotes(v: str) -> str:
	# Strip an unquoted YAML inline comment (` #...`) before quote handling.
	# A bare `#` with no leading space is part of the value (e.g. aqua names
	# like `_go/sigsum.org/sigsum-go#cmd/sigsum-key`).
	v = v.split(' #', 1)[0].strip()
	if len(v) >= 2 and v[0] == v[-1] and v[0] in ("'", '"'):
		return v[1:-1]
	return v


def parse(path: str) -> dict[str, tuple[str, str]]:
	"""Return {canonical_id: (github_repo_or_empty, package_block_text)}.

	github_repo is set when the package has repo_owner+repo_name (so the id
	resolves to a github URL); empty for name-only or path-only packages.
	"""
	with open(path) as f:
		text = f.read()
	# Top-level packages start with '  - ' at column 0. Split on those boundaries.
	parts = re.split(r'(?m)^  - ', text)
	pkgs: dict[str, tuple[str, str]] = {}
	for body in parts[1:]:
		block = '  - ' + body
		# Top-level package fields are at exactly 4-space indent. The first
		# field also appears inline on the '  - ' line itself.
		fields: dict[str, str] = {}
		first_line, _, rest = body.partition('\n')
		m = re.match(r'(name|repo_owner|repo_name|path|type):\s*(.*?)\s*$', first_line)
		if m:
			fields[m.group(1)] = strip_quotes(m.group(2))
		for ln in rest.splitlines():
			mm = re.match(r'^    (name|repo_owner|repo_name|path):\s*(.*?)\s*$', ln)
			if mm:
				fields.setdefault(mm.group(1), strip_quotes(mm.group(2)))
		github_repo = ''
		if fields.get('repo_owner') and fields.get('repo_name'):
			github_repo = f"{fields['repo_owner']}/{fields['repo_name']}"
		if fields.get('name'):
			pkg_id = fields['name']
		elif github_repo:
			pkg_id = github_repo
		elif fields.get('path'):
			pkg_id = fields['path']
		else:
			continue
		pkgs[pkg_id] = (github_repo, block)
	return pkgs


old = parse(old_path)
new = parse(new_path)

added = sorted(set(new) - set(old))
updated = sorted(k for k in (set(old) & set(new)) if old[k][1] != new[k][1])

if not added and not updated:
	sys.exit(0)


def link(pkg: str, github_repo: str) -> str:
	if github_repo:
		return f'[`{pkg}`](https://github.com/{github_repo})'
	return f'`{pkg}`'


sub = heading + '#'
out: list[str] = []
out.append(f'{heading} 📦 Aqua Registry Updates')
out.append('')
if added:
	out.append(f'{sub} New Packages ({len(added)})')
	out.append('')
	out.extend(f'- {link(p, new[p][0])}' for p in added)
	out.append('')
if updated:
	out.append(f'{sub} Updated Packages ({len(updated)})')
	out.append('')
	out.extend(f'- {link(p, new[p][0])}' for p in updated)
	out.append('')

print('\n'.join(out))
PYEOF
