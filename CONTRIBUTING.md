# Contributing

Read the [contributing guide](https://mise.jdx.dev/contributing.html) before you
open a pull request. For documentation changes, also read
[Working on the mise docs](docs/README.md).

## mbx build cache

mise wraps `cargo` with [mbx](https://mr-boxington.jdx.dev), so compiled work is shared across
checkouts. `mise run` tasks and `mise exec -- cargo …` use the wrapper; plain `cargo` does too once
mise is [activated in your shell](https://mise.jdx.dev/shell-setup.html). Builds that set
`MBX_DISABLE=1`, as most CI jobs do, skip the cache.
