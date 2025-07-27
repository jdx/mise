#!/usr/bin/env bash

set -euo pipefail

echo "🚀 Node.js Performance Comparison: Direct vs mise vs DotSlash"
echo "=============================================================="

cargo b --profile=release

# Create temporary directory for the test
TEMP_DIR=$(mktemp -d)
echo "📁 Working in temporary directory: $TEMP_DIR"
cp target/release/mise "$TEMP_DIR"
cd "$TEMP_DIR"
PATH="$TEMP_DIR:$PATH"

# Set up mise config for dependencies
cat >.mise.toml <<'EOF'
[tools]
hyperfine = "latest"
"ubi:facebook/dotslash" = "latest"
node = "20.0.0"
EOF

echo "📦 Installing dependencies..."
mise install

# Add tools to PATH
PATH="$(mise where hyperfine)/bin:$PATH"
PATH="$(mise where ubi:facebook/dotslash):$PATH"
PATH="$(mise where node)/bin:$PATH"

# Verify installations
echo "🔍 Verifying installations..."
echo "  Node.js: $(node --version)"
echo "  DotSlash: $(dotslash --version 2>/dev/null || echo "Failed")"
echo "  Hyperfine: $(hyperfine --version | head -1)"

# Create test directory
mkdir -p bin

# Get direct node path
DIRECT_NODE_PATH="$(which node)"
echo "🎯 Direct Node.js path: $DIRECT_NODE_PATH"

# Create mise tool stub
cat >bin/node-mise <<'EOF'
#!/usr/bin/env -S mise tool-stub
version = "20.0.0"
tool = "node"
bin = "node"
EOF
chmod +x bin/node-mise

# Test mise tool stub works
echo "📋 Testing mise tool stub..."
./bin/node-mise --version

# Create DotSlash shim
cat >bin/node-dotslash <<'EOF'
#!/usr/bin/env dotslash

{
  "name": "node-v20.0.0",
  "platforms": {
    "linux-x86_64": {
      "size": 45952734,
      "hash": "blake3",
      "digest": "39fcc8b488ae4877b99ddf40603e9808bb73885742b48401f136f16304615c83",
      "format": "tar.gz",
      "path": "node-v20.0.0-linux-x64/bin/node",
      "providers": [
        {
          "url": "https://nodejs.org/dist/v20.0.0/node-v20.0.0-linux-x64.tar.gz"
        }
      ]
    },
	"macos-aarch64": {
		"size": 41339150,
		"hash": "blake3",
		"digest": "1373835099da2743cc18f136e54bc5c08d91f5234ec2f313336d7b940d815c4b",
		"format": "tar.gz",
		"path": "node-v20.0.0-darwin-arm64/bin/node",
		"providers": [
			{
				"url": "https://nodejs.org/dist/v20.0.0/node-v20.0.0-darwin-arm64.tar.gz"
			}
		]
	}
  }
}
EOF
chmod +x bin/node-dotslash

# Test DotSlash shim works
echo "📋 Testing DotSlash shim..."
./bin/node-dotslash --version

echo ""
echo "⚡ Running Performance Benchmark..."

# Run the performance comparison
hyperfine \
	--warmup 5 \
	--min-runs 20 \
	--export-markdown results.md \
	--export-json results.json \
	--command-name "Direct Node.js" "$DIRECT_NODE_PATH --version" \
	--command-name "Mise Shim" "./bin/node-mise --version" \
	--command-name "DotSlash Shim" "./bin/node-dotslash --version"

echo ""
echo "📈 Results:"
cat results.md

# Calculate overhead if jq and bc are available
if command -v jq &>/dev/null && command -v bc &>/dev/null; then
	echo ""
	echo "📊 Overhead Analysis:"

	direct_time=$(jq -r '.results[0].mean' results.json)
	mise_time=$(jq -r '.results[1].mean' results.json)
	dotslash_time=$(jq -r '.results[2].mean' results.json)

	mise_overhead=$(echo "scale=1; ($mise_time - $direct_time) / $direct_time * 100" | bc)
	dotslash_overhead=$(echo "scale=1; ($dotslash_time - $direct_time) / $direct_time * 100" | bc)

	echo "  Direct Node.js:     ${direct_time}s (baseline)"
	echo "  Mise overhead:      +${mise_overhead}%"
	echo "  DotSlash overhead:  +${dotslash_overhead}%"

	# File sizes
	echo ""
	echo "📝 File Sizes:"
	echo "  Direct Node.js:     $(du -h "$DIRECT_NODE_PATH" | cut -f1)"
	echo "  Mise shim:          $(du -h bin/node-mise | cut -f1)"
	echo "  DotSlash shim:      $(du -h bin/node-dotslash | cut -f1)"
fi

echo ""
echo "✅ Performance comparison complete!"
echo "🗂️  Test files are in: $TEMP_DIR"
echo "🧹 To clean up: rm -rf $TEMP_DIR"
