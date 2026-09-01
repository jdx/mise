# Contributing

See the [contributing guide](https://mise.jdx.dev/contributing).

## mbx build cache

`mise install` installs [mbx](https://mr-boxington.jdx.dev) 1.2 and activates
its transparent Cargo shim. The normal `mise run build`, `mise run test:unit`,
and `mise run lint` workflows therefore use the cache while invoking Cargo
normally. To bypass mbx without skipping or weakening a check, prefix the
equivalent Cargo command with `MBX_DISABLE=1`:

```sh
MBX_DISABLE=1 cargo build --all-features
MBX_DISABLE=1 cargo test --all-features
MBX_DISABLE=1 cargo check --all-features
```

If bypassed Cargo succeeds where the shim fails, or mbx introduces a papercut, please start a
[mr-boxington Discussion](https://github.com/jdx/mr-boxington/discussions).
Include the repository and commit, operating system, `mbx --version`,
`mbx doctor`, and both commands and their output. Before posting, redact
secrets, absolute cache paths, remote URLs, namespaces, and other sensitive or
identifying details.
