---
description: "mise can install and manage multiple versions of Bare on the same system."
---

# Bare

`mise` can install and manage multiple versions of the
[Bare JavaScript runtime](https://github.com/holepunchto/bare) on the same system.

## Usage

Install Bare for the current project and verify the selected executable:

```sh
mise use bare@latest
mise exec -- bare --version
```

Use `mise use -g bare@latest` for a personal default outside projects. See available
versions with `mise ls-remote bare`.

These commands use mise's built-in Bare support and do not require Node.js or npm.
mise downloads the standalone executables published by
[`bare-runtime`](https://github.com/holepunchto/bare-runtime/releases) and verifies
the SHA-256 digest provided by GitHub.

The version list includes releases that provide standalone runtime artifacts. The newest
runtime release can briefly trail the newest `bare` npm package while upstream publishes
the corresponding binaries.

## Platforms

The built-in backend supports the upstream desktop artifacts:

| Platform                    | Architectures |
| --------------------------- | ------------- |
| Linux (glibc 2.35 or newer) | x64, ARM64    |
| macOS                       | x64, ARM64    |
| Windows                     | x64, ARM64    |

Upstream builds Linux releases on Ubuntu 22.04 and does not publish musl artifacts.

An installed external plugin with the same name can change this behavior. Use
`mise plugins ls` to check for overrides. See the
[core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/bare.rs)
for backend details.
