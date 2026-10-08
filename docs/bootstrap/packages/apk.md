---
description: "Install Alpine Linux packages with apk from mise.toml, including version pins."
---

# Alpine packages (apk)

The `apk` manager installs Alpine Linux packages with `apk add`. It uses
[sudo](/bootstrap/packages/#sudo) when mise is not running as root.

```toml
[bootstrap.packages]
"apk:build-base" = "latest"
"apk:zlib-dev" = "1.3.1-r2" # version pin
```

```sh
mise bootstrap packages apply --manager apk --dry-run
mise bootstrap packages apply --manager apk
```

## Prerequisites

The manager is available on Linux when `apk` is on `PATH`. On other machines,
its entries show as [`skipped`](/bootstrap/packages/#choose-platforms).

## Package names

Use the package name as `apk add` takes it, such as `build-base` or
`zlib-dev`. Run `apk search <name>` to find one.

## Version pins

mise passes a pin to apk as `name=version`. The `1.3.1-r2` pin above is only an
example. Run `apk policy zlib-dev` on the target to list the versions its
repositories offer. A pin cannot reach a version that is no longer in those
repositories, and mise does not add an older Alpine repository.

## What mise runs

| Operation                       | Command                                                |
| ------------------------------- | ------------------------------------------------------ |
| Check installed state (no sudo) | `apk info -e -v <packages>`                            |
| Install                         | `apk add -- <packages>`                                |
| `apply --update`                | `apk add --update-cache -- <packages>`                 |
| Upgrade                         | `apk upgrade --available --update-cache -- <packages>` |

## Remove packages

mise does not remove apk packages. Deleting an entry leaves the package
installed; run `apk del` yourself.
