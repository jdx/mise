# vfox

A Rust runtime for [vfox](https://github.com/version-fox/vfox) plugins. It loads
a plugin's Lua hooks and calls them to list a tool's versions, install a
version, and compute its environment variables, without the `vfox` CLI.

[mise](https://mise.jdx.dev) uses this crate for its
[vfox backend](https://mise.jdx.dev/dev-tools/backends/vfox.html) and for
[tool plugins](https://mise.jdx.dev/tool-plugin-development.html) and
[backend plugins](https://mise.jdx.dev/backend-plugin-development.html). The
crate is published for mise; its API may change between releases.

The plugins under `embedded-plugins/` are copies of upstream plugins that are
compiled into the crate. Fix a plugin in its own repository, not here.

## Development

Run the crate's tests and lints from the repository root:

```sh
mise --cd crates/vfox run test
mise --cd crates/vfox run lint
```
