#!/usr/bin/env bash
set -euxo pipefail

error() {
	echo "$@" >&2
	exit 1
}

if [[ -z "${NODE_AUTH_TOKEN:-}" ]]; then
	echo "NODE_AUTH_TOKEN must be set" >&2
	exit 0
fi

mkdir -p "$RELEASE_DIR/npm"

dist_tag_from_version() {
	IFS="-" read -r -a version_split <<<"$1"
	IFS="." read -r -a version_split <<<"${version_split[1]:-latest}"
	echo "${version_split[0]}"
}
dist_tag="$(dist_tag_from_version "$RTX_VERSION")"

platforms=(
	linux-x64
	linux-arm64
	macos-x64
	macos-arm64
)
for platform in "${platforms[@]}"; do
	# shellcheck disable=SC2206
	platform_split=(${platform//-/ })
	os="${platform_split[0]}"
	arch="${platform_split[1]}"

	if [[ "$os" == "macos" ]]; then
		os="darwin"
	fi

	cp "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform.tar.gz" "$RELEASE_DIR/rtx-latest-$platform.tar.gz"
	cp "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform.tar.xz" "$RELEASE_DIR/rtx-latest-$platform.tar.xz"
	tar -xzvf "$RELEASE_DIR/rtx-latest-$platform.tar.gz" -C "$RELEASE_DIR"
	rm -rf "$RELEASE_DIR/npm"
	mv "$RELEASE_DIR/rtx" "$RELEASE_DIR/npm"
	cat <<EOF >"$RELEASE_DIR/npm/package.json"
{
  "name": "$NPM_PREFIX-$os-$arch",
  "version": "$RTX_VERSION",
  "description": "polyglot runtime manager",
  "bin": {
    "rtx": "bin/rtx"
  },
  "repository": {
    "type": "git",
    "url": "https://github.com/jdx/rtx"
  },
  "files": [
    "bin",
    "README.md"
  ],
  "license": "MIT",
  "os": "$os",
  "cpu": "$arch"
}
EOF
	pushd "$RELEASE_DIR/npm"
	tree || true
	npm publish --access public --tag "$dist_tag"
	popd
done

cat <<EOF >"$RELEASE_DIR/npm/installArchSpecificPackage.js"
var spawn = require('child_process').spawn;
var path = require('path');
var fs = require('fs');

function installArchSpecificPackage(version) {

    process.env.npm_config_global = 'false';

    var platform = process.platform == 'win32' ? 'win' : process.platform;
    var arch = platform == 'win' && process.arch == 'ia32' ? 'x86' : process.arch;

    var cp = spawn(platform == 'win' ? 'npm.cmd' : 'npm', ['install', '--no-save', ['$NPM_PREFIX', platform, arch].join('-') + '@' + version], {
        stdio: 'inherit',
        shell: true
    });

    cp.on('close', function(code) {
        var pkgJson = require.resolve(['$NPM_PREFIX', platform, arch].join('-') + '/package.json');
        var subpkg = JSON.parse(fs.readFileSync(pkgJson, 'utf8'));
        var executable = subpkg.bin.rtx;
        var bin = path.resolve(path.dirname(pkgJson), executable);

        try {
            fs.mkdirSync(path.resolve(process.cwd(), 'bin'));
        } catch (e) {
            if (e.code != 'EEXIST') {
                throw e;
            }
        }

        linkSync(bin, path.resolve(process.cwd(), executable));

        if (platform == 'win') {
            var pkg = JSON.parse(fs.readFileSync(path.resolve(process.cwd(), 'package.json')));
            fs.writeFileSync(path.resolve(process.cwd(), 'bin/rtx'), 'This file intentionally left blank');
            pkg.bin.rtx = 'bin/rtx.exe';
            fs.writeFileSync(path.resolve(process.cwd(), 'package.json'), JSON.stringify(pkg, null, 2));
        }

        return process.exit(code);

    });
}

function linkSync(src, dest) {
    try {
        fs.unlinkSync(dest);
    } catch (e) {
        if (e.code != 'ENOENT') {
            throw e;
        }
    }
    return fs.linkSync(src, dest);
}

const pjson = require('./package.json')
installArchSpecificPackage(pjson.version)
EOF

cat <<EOF >"$RELEASE_DIR/npm/package.json"
{
  "name": "$NPM_PREFIX",
  "description": "polyglot runtime manager",
  "version": "$RTX_VERSION",
  "repository": {
    "type": "git",
    "url": "https://github.com/jdx/rtx"
  },
  "files": [
    "installArchSpecificPackage.js",
    "README.md"
  ],
  "scripts": {
    "prepack": "rm -rf bin",
    "preinstall": "node installArchSpecificPackage.js"
  },
  "bin": {
    "rtx": "./bin/rtx"
  },
  "license": "MIT",
  "engines": {
    "node": ">=5.0.0"
  }
}
EOF
pushd "$RELEASE_DIR/npm"
npm publish --access public --tag "$dist_tag"
popd
