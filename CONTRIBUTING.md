# Contributing

See the [contributing guide](https://mise.jdx.dev/contributing).

> [!CAUTION]
> **AI replies to Discussions and Issues are restricted.** Only use AI to reply to a thread if you
> created it, opened a PR that fixes it, or have already had a contribution, attributed to your GitHub account, merged into the default branch of mise.
> Everyone else is not allowed to, including with lightly edited, reviewed, or disclosed model
> output. Doing this is an instant ban across all of jdx's projects. See
> [Community Participation](https://mise.jdx.dev/contributing#community-participation).

## mbx build cache

mise wraps `cargo` with [mbx](https://mr-boxington.jdx.dev), so compiled work is shared across
checkouts. `mise run` tasks and `mise exec -- cargo …` use the wrapper; plain `cargo` does too once
mise is [activated in your shell](https://mise.jdx.dev/getting-started.html#activate-mise). Builds
that set `MBX_DISABLE=1`, as most CI jobs do, skip the cache.
