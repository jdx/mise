#!/bin/bash

# Setup script for mise development shim
# This creates a shim that allows running mise via 'cargo run'

echo "Setting up mise development shim..."

cat > /usr/local/bin/mise << 'EOF'
#!/bin/bash

# Mise development shim
# This script allows running the development version of mise via 'cargo run'

# Change to the mise project directory
cd /workspaces/mise

# Run cargo with all arguments passed through
exec cargo run -- "$@"
EOF

chmod +x /usr/local/bin/mise

echo "Mise development shim created at /usr/local/bin/mise"
echo "You can now run 'mise' commands which will use 'cargo run' under the hood" 
