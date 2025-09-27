#!/bin/bash

# Setup script for mise development shim
# This creates a shim that allows running mise via 'cargo run'

# Install OpenSSL development libraries if they're missing
if ! pkg-config --exists openssl 2>/dev/null; then
    echo "Installing OpenSSL development libraries..."
    apt-get update && apt-get install -y libssl-dev
fi

echo "Ensuring Rust is up to date for edition 2024 support..."

# Install rustup if not already installed
if ! command -v rustup &> /dev/null; then
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Update to latest stable (1.83+ supports edition 2024)
rustup update stable
rustup default stable

echo "Setting up mise development shim..."

cat > /usr/local/bin/mise << 'EOF'
#!/bin/bash

# Mise development shim
# This script allows running the development version of mise via 'cargo run'

# Ensure cargo is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Find the mise project directory - handle both /workspaces and /workspace
if [ -d "/workspaces/mise" ]; then
    MISE_DIR="/workspaces/mise"
elif [ -d "/workspace" ]; then
    MISE_DIR="/workspace"
else
    echo "Error: Could not find mise project directory"
    exit 1
fi

# Run cargo with all arguments passed through
# Using --manifest-path allows the command to work from any directory
exec cargo run --all-features --manifest-path "$MISE_DIR/Cargo.toml" -- "$@"
EOF

chmod +x /usr/local/bin/mise

echo "Mise development shim created at /usr/local/bin/mise"
echo "You can now run 'mise' commands which will use 'cargo run' under the hood"

# Install mise dependencies in the project directory
echo "Installing mise dependencies..."
if [ -d "/workspaces/mise" ]; then
    cd /workspaces/mise && mise install
elif [ -d "/workspace" ]; then
    cd /workspace && mise install
else
    echo "Warning: Could not find project directory to install dependencies"
fi
