#!/usr/bin/env fish
# Test that mise activate --shims reorders shims to front of PATH
# even when shims are already present (e.g. VS Code re-sourcing config.fish)
# See: https://github.com/jdx/mise/discussions/6072

# Get the mise binary path from argv (passed by the wrapper)
set -l mise_dir $argv[1]

# Ensure shims directory exists (fish_add_path skips non-existent dirs)
mkdir -p $HOME/.local/share/mise/shims

# Clear any pre-existing __MISE_BIN so the debug binary uses current_exe()
set -e __MISE_BIN

# Simulate VS Code scenario: shims already in PATH but at low priority
# (after /opt/homebrew/bin, like VS Code would arrange it)
set -gx PATH $mise_dir /opt/homebrew/bin /usr/bin $HOME/.local/share/mise/shims /bin

# Helper to find path indices
function find_path_indices
    set -l shims_idx 0
    set -l brew_idx 0
    for i in (seq (count $PATH))
        if test "$PATH[$i]" = "$HOME/.local/share/mise/shims"
            set shims_idx $i
        end
        if test "$PATH[$i]" = "/opt/homebrew/bin"
            set brew_idx $i
        end
    end
    echo $shims_idx $brew_idx
end

# Verify shims are initially after /opt/homebrew/bin
set -l indices (find_path_indices)
set -l shims_idx $indices[1]
set -l brew_idx $indices[2]

if test $shims_idx -le $brew_idx
    echo "FAIL: shims should start after homebrew for this test to be meaningful"
    echo "PATH: $PATH"
    exit 1
end

echo "Before: shims at index $shims_idx, brew at index $brew_idx (shims after brew)"

# Now activate with --shims using the mise binary, simulating config.fish re-source
$mise_dir/mise activate fish --shims | source

# Check that shims are now before /opt/homebrew/bin
set indices (find_path_indices)
set shims_idx $indices[1]
set brew_idx $indices[2]

if test $shims_idx -eq 0
    echo "FAIL: shims not found in PATH after activate --shims"
    echo "PATH: $PATH"
    exit 1
end

if test $shims_idx -ge $brew_idx
    echo "FAIL: shims (index $shims_idx) should be before homebrew (index $brew_idx) after activate --shims"
    echo "PATH: $PATH"
    exit 1
end

echo "After: shims at index $shims_idx, brew at index $brew_idx (shims before brew)"
echo "SUCCESS: shims reordered to front of PATH"
