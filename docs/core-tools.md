# Core Tools

Core tools have installation logic built into mise. They do not require a separately
installed plugin. Their language guides explain platform support, compilation options,
virtual environments, and other runtime-specific behavior.

List the current core entries with:

```sh
mise registry -b core
```

## Language guides

- [Bun](/lang/bun)
- [Deno](/lang/deno)
- [.NET](/lang/dotnet)
- [Elixir](/lang/elixir)
- [Erlang](/lang/erlang)
- [Go](/lang/go)
- [Java](/lang/java)
- [Node.js](/lang/node)
- [Python](/lang/python)
- [Ruby](/lang/ruby)
- [Rust](/lang/rust)
- [Swift](/lang/swift)
- [Zig](/lang/zig)

## Selecting another implementation

Installing an external plugin with the same name can override a core tool. Use that only
when you need behavior provided by the plugin; it also changes the installation and trust
requirements. See [plugins](/plugins.html) and [backend selection](/dev-tools/backends/)
for how mise chooses an implementation.

To select the built-in implementation explicitly, use the `core:` prefix, for example
`mise use core:python@3.14`. The [registry](/registry.html) covers tools beyond the core set.
