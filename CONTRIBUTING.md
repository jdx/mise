# Contributing

See the [contributing guide](https://mise.jdx.dev/contributing).

## mbx build cache

mise wraps `cargo` with [mbx](https://mr-boxington.jdx.dev), so compiled work is shared across
checkouts. `mise run` tasks and `mise exec -- cargo …` use the wrapper; plain `cargo` does too once
mise is [activated in your shell](https://mise.jdx.dev/getting-started.html#activate-mise). Builds
that set `MBX_DISABLE=1`, as most CI jobs do, skip the cache.
