# vfox-npm

A vfox plugin for installing npm packages as tools.

## Installation

```bash
mise plugin install vfox-npm https://github.com/jdx/vfox-npm
```

## Usage

This plugin allows you to install npm packages as tools using the `vfox-npm:package` format.

### Examples

```bash
# Install prettier
mise install vfox-npm:prettier@latest

# Use prettier
mise x vfox-npm:prettier -- prettier --version

# Install specific version
mise install vfox-npm:eslint@8.0.0

# List available versions
mise ls-remote vfox-npm:prettier
```

## How it works

This plugin implements the vfox backend interface to:

1. **List versions**: Fetches available versions from the npm registry
2. **Install packages**: Uses `npm install` to install packages locally
3. **Set environment**: Adds `node_modules/.bin` to PATH for binary access

## Requirements

- Node.js and npm must be installed on your system
- This plugin requires the latest version of mise with vfox backend support

## License

MIT
