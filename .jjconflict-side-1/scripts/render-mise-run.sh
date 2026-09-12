#!/usr/bin/env bash
# shellcheck disable=SC2016
set -euxo pipefail

BASE_DIR="$(pwd)"

# Create the mise.run directory in releases
mkdir -p "artifacts/mise.run"

# Function to generate shell-specific script
generate_shell_script() {
	local shell_name="$1"
	local config_var_name="$2"
	local config_file_var="$3"
	local config_setup_commands="$4"
	local activation_command="$5"
	local source_command="$6"
	local final_message="$7"

	echo "Generating $shell_name script..."

	# Export variables for envsubst
	export SHELL_NAME="$shell_name"
	export CONFIG_VAR_NAME="$config_var_name"
	export CONFIG_FILE_VAR="$config_file_var"
	export CONFIG_SETUP_COMMANDS="$config_setup_commands"
	export ACTIVATION_COMMAND="$activation_command"
	export SOURCE_COMMAND="$source_command"
	export FINAL_MESSAGE="$final_message"

	# Generate script from template
	envsubst '$SHELL_NAME,$CONFIG_VAR_NAME,$CONFIG_FILE_VAR,$CONFIG_SETUP_COMMANDS,$ACTIVATION_COMMAND,$SOURCE_COMMAND,$FINAL_MESSAGE' <"$BASE_DIR/packaging/mise.run/shell.envsubst" >"artifacts/mise.run/$shell_name"

	# Make executable
	chmod +x "artifacts/mise.run/$shell_name"

	# Validate with shellcheck
	shellcheck "artifacts/mise.run/$shell_name"
}

# Generate zsh script
generate_shell_script "zsh" \
	'zshrc="${ZDOTDIR-$HOME}/.zshrc"' \
	"zshrc" \
	'if [ ! -f "$zshrc" ]; then
    touch "$zshrc"
  fi' \
	'echo "eval \"\$($install_path activate zsh)\" # added by https://mise.run/zsh" >> "$zshrc"' \
	'source $zshrc' \
	"restart your shell or run 'source \${ZDOTDIR-\$HOME}/.zshrc' to start using mise"

# Generate bash script
generate_shell_script "bash" \
	'bashrc="$HOME/.bashrc"' \
	"bashrc" \
	'if [ ! -f "$bashrc" ]; then
    touch "$bashrc"
  fi' \
	'echo "eval \"\$($install_path activate bash)\" # added by https://mise.run/bash" >> "$bashrc"' \
	"source ~/.bashrc" \
	"restart your shell or run 'source ~/.bashrc' to start using mise"

# Generate fish script
generate_shell_script "fish" \
	'fish_config_dir="$HOME/.config/fish"
  fish_config="$fish_config_dir/config.fish"' \
	"fish_config" \
	'if [ ! -d "$fish_config_dir" ]; then
    mkdir -p "$fish_config_dir"
  fi
  if [ ! -f "$fish_config" ]; then
    touch "$fish_config"
  fi' \
	'echo "$install_path activate fish | source # added by https://mise.run/fish" >> "$fish_config"' \
	"source ~/.config/fish/config.fish" \
	"restart your fish shell or run 'source ~/.config/fish/config.fish' to start using mise"

echo "Shell scripts generated successfully in artifacts/mise.run/"
