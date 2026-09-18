---
description: "Build and install Go command-line packages with go install."
---

# Go Backend

The `go` backend builds Go command-line packages with `go install`. Use the
import path of the executable package, which may include `/cmd/TOOL` or a major
version suffix such as `/v4`. Libraries belong in your application's `go.mod`.

The code for this is inside the mise repository at [`./src/backend/go.rs`](https://github.com/jdx/mise/blob/main/src/backend/go.rs).

## Dependencies

Install Go and the tool in the current project. The configured Go installation
is available while mise builds the dependent tool:

```sh
mise use go@1.26 go:github.com/DarthSim/hivemind
mise exec -- hivemind --help
```

This records both tools in `mise.toml`; add `-g` for global configuration.
Source builds may also need Git, a C compiler, or native libraries, depending on
the package and whether it uses cgo.

## Usage

List available versions with `mise ls-remote go:github.com/DarthSim/hivemind`.
To select one, run `mise use go:github.com/DarthSim/hivemind@VERSION`, replacing
`VERSION` with a listed release. mise writes the resulting executable into its
own installation directory instead of your ordinary `GOBIN`.

### Private modules

Private modules use Go's normal VCS authentication. Export `GOPRIVATE`, or
define it in mise's `[env]` configuration, so mise delegates version discovery
to Go instead of querying the public module proxy itself. Values set only with
`go env -w` are not read by mise when choosing the discovery path:

```toml
[env]
GOPRIVATE = "github.com/acme/*"
```

Go uses `GOPRIVATE` as the default for both `GONOPROXY` and `GONOSUMDB`. If you
configure those variables separately, set each one according to the proxy and
checksum-database privacy you need.

mise caps how many versions of a module it reports release dates for, on either
route: the newest 100 through a module proxy, and the newest 10 when it has to
reach the module over VCS, where each date costs its own round trip. That cap
shows up in `mise ls-remote`, which lists the older versions without a date.
[`minimum_release_age`](/configuration/settings.html#minimum_release_age) still
applies exactly, on both routes: before mise settles on a version it has no date
for, it reads that one version's date and moves further back while the answer is
newer than the cutoff. A cutoff deep enough to reach past the dated versions
therefore costs one extra query per version it skips, which is why a long cutoff
such as `90d` can make the first resolution of a module with many releases
noticeably slower over VCS. Requesting `latest` usually avoids all of it — mise
asks for `latest` directly, in one query — though when that answer is itself
newer than the cutoff, mise falls back to the version list and dates candidates
from there.

You can also pin a specific Go module version, including an unreleased
pseudo-version:

```toml
[tools]
"go:github.com/grafana/oats" = "v0.7.1-0.20260703092802-96201f1b8136"
```

If you need to resolve an unreleased revision directly from VCS instead of the
module proxy, combine the pinned version with [`install_env`](/dev-tools/backends/go.html#install-env):

```toml
[tools]
"go:github.com/grafana/oats" = { version = "v0.7.1-0.20260703092802-96201f1b8136", install_env = { GOPROXY = "direct", GONOSUMDB = "github.com/grafana/oats" } }
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `go` backend—these
go in `[tools]` in `mise.toml`.

### `install_env`

Set environment variables for the `go install` command. mise still sets `GOBIN`
to the tool install directory after applying `install_env`. Put `GOPRIVATE` in
`[env]` as shown above when it must also affect version discovery.

```toml
[tools]
"go:github.com/acme/my-tool" = { version = "latest", install_env = { GOPRIVATE = "github.com/acme/*" } }
```

### `tags`

Specify Go build tags (passed as `go install -tags`):

```toml
[tools]
"go:github.com/golang-migrate/migrate/v4/cmd/migrate" = { version = "latest", tags = "postgres" }
# equivalent array form:
# "go:github.com/golang-migrate/migrate/v4/cmd/migrate" = { version = "latest", tags = ["postgres", "mysql"] }
```

## Troubleshooting

- **Package is not a main package:** use the executable's import path, not the repository root or a library package.
- **Private module lookup fails:** check exported `GOPRIVATE` and your Go/Git credentials; mise's GitHub token is not a substitute for VCS authentication.
- **Go version or compiler error:** use a toolchain supported by the package and install any required native build dependencies.
