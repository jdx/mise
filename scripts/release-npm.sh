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
dist_tag="$(dist_tag_from_version "$MISE_VERSION")"

platforms=(
  linux-x64
  linux-x64-musl
  linux-arm64
  linux-arm64-musl
  linux-armv6
  linux-armv6-musl
  linux-armv7
  linux-armv7-musl
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

  cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform.tar.gz" "$RELEASE_DIR/mise-latest-$platform.tar.gz"
  cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform.tar.xz" "$RELEASE_DIR/mise-latest-$platform.tar.xz"
  tar -xzvf "$RELEASE_DIR/mise-latest-$platform.tar.gz" -C "$RELEASE_DIR"
  rm -rf "$RELEASE_DIR/npm"
  mv "$RELEASE_DIR/mise" "$RELEASE_DIR/npm"
  cat <<EOF >"$RELEASE_DIR/npm/package.json"
{
  "name": "$NPM_PREFIX-$os-$arch",
  "version": "$MISE_VERSION",
  "description": "polyglot runtime manager",
  "bin": {
    "mise": "bin/mise"
  },
  "repository": {
    "type": "git",
    "url": "https://github.com/jdx/mise"
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
  if [ "$DRY_RUN" != "0" ]; then
    echo DRY_RUN
    echo npm publish --access public --tag "$dist_tag"
    echo DRY_RUN
  else
    npm publish --access public --tag "$dist_tag" || true
  fi
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
        var executable = subpkg.bin.mise;
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
            fs.writeFileSync(path.resolve(process.cwd(), 'bin/mise'), 'This file intentionally left blank');
            pkg.bin.mise = 'bin/mise.exe';
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
  "version": "$MISE_VERSION",
  "repository": {
    "type": "git",
    "url": "https://github.com/jdx/mise"
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
    "mise": "./bin/mise"
  },
  "license": "MIT",
  "engines": {
    "node": ">=5.0.0"
  }
}
EOF
pushd "$RELEASE_DIR/npm"
if [ "$DRY_RUN" != "0" ]; then
  echo DRY_RUN
  echo npm publish --access public --tag "$dist_tag"
  echo DRY_RUN
else
  npm publish --access public --tag "$dist_tag" || true
fi
popd
