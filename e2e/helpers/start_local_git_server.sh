#!/usr/bin/env bash
set -euo pipefail

# Helper script to create a local git repository for faster tests
# This makes the git remote task tests faster by using a local file:// URL instead of remote URLs

LOCAL_GIT_REPO_DIR="${TEST_TMPDIR:-${TMPDIR:-/tmp}}/local_mise_repo.git"

setup_local_git_server() {
	# Save current working directory
	local original_pwd
	original_pwd="$(pwd)"

	# Create a local bare git repository with just the files we need for tests
	if [[ ! -d $LOCAL_GIT_REPO_DIR ]]; then
		mkdir -p "$LOCAL_GIT_REPO_DIR"
		cd "$LOCAL_GIT_REPO_DIR"
		git init --bare

		# Initialize with a temporary working tree to create initial commit
		local temp_work_dir="${LOCAL_GIT_REPO_DIR}_work"
		git clone "$LOCAL_GIT_REPO_DIR" "$temp_work_dir"
		cd "$temp_work_dir"

		# Copy the specific files we need for tests
		mkdir -p xtasks/lint
		if [[ -f "$ROOT/xtasks/lint/ripgrep" ]]; then
			cp "$ROOT/xtasks/lint/ripgrep" xtasks/lint/ripgrep
		else
			# If the file doesn't exist, create a simple script
			cat >xtasks/lint/ripgrep <<'EOF'
#!/usr/bin/env bash
echo "Running local ripgrep task"
EOF
			chmod +x xtasks/lint/ripgrep
		fi

		# Create initial commit
		git add .
		git commit -m "Initial commit with test files"

		# Create the v2025.1.17 tag for tests that need a specific ref
		git tag v2025.1.17

		# Push back to bare repository
		git push origin master
		git push origin v2025.1.17

		# Clean up temporary working directory
		cd "$original_pwd"
		rm -rf "$temp_work_dir"
	fi

	# Restore original working directory
	cd "$original_pwd"

	debug "Local git repository created at $LOCAL_GIT_REPO_DIR"
}

stop_local_git_server() {
	# No server to stop, this is a no-op for file-based approach
	:
}

get_local_git_url() {
	echo "file://$LOCAL_GIT_REPO_DIR"
}
