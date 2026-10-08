---
description: "Install Node.js, Python, Go, Java and other languages with installers built into mise, no plugin needed."
---

# Core tools overview

Core tools are languages whose installers are built into mise, so they need no
plugin. Each guide covers platform support, how mise installs the language,
version files, and language-specific options.

```sh
mise use node@24 python@3.14
```

| Tool                        | Name in `mise.toml` | Version files (when [enabled](/dev-tools/versions.html#idiomatic-version-files)) |
| --------------------------- | ------------------- | -------------------------------------------------------------------------------- |
| [Bun](/lang/bun.html)       | `bun`               | `.bun-version`, `package.json`                                                   |
| [Deno](/lang/deno.html)     | `deno`              | `.deno-version`, `package.json`                                                  |
| [.NET](/lang/dotnet.html)   | `dotnet`            | `global.json`                                                                    |
| [Elixir](/lang/elixir.html) | `elixir`            | `.exenv-version`                                                                 |
| [Erlang](/lang/erlang.html) | `erlang`            |                                                                                  |
| [Go](/lang/go.html)         | `go`                | `.go-version`, `go.mod`, `go.work`                                               |
| [Java](/lang/java.html)     | `java`              | `.java-version`, `.sdkmanrc`                                                     |
| [Node.js](/lang/node.html)  | `node`              | `.nvmrc`, `.node-version`, `package.json`                                        |
| [Python](/lang/python.html) | `python`            | `.python-version`, `.python-versions`                                            |
| [Ruby](/lang/ruby.html)     | `ruby`              | `.ruby-version`, `Gemfile`                                                       |
| [Rust](/lang/rust.html)     | `rust`              | `rust-toolchain.toml`                                                            |
| [Swift](/lang/swift.html)   | `swift`             | `.swift-version`                                                                 |
| [Zig](/lang/zig.html)       | `zig`               | `.zig-version`                                                                   |

`mise registry -b core` lists the same set, and `dotnet-core` is another name
for `dotnet`. For tools beyond these languages, see the
[registry](/registry.html).

## Selecting another implementation {#selecting-another-implementation}

An installed plugin with the same name as a core tool can take precedence over
the built-in installer. Use one only when you need behavior the plugin provides,
because it also changes how the tool installs and whose code you trust. See
[plugins](/plugins.html) and
[how backend selection works](/dev-tools/backends/#how-backend-selection-works).

To use the built-in installer explicitly, prefix the name with `core:`, as in
`mise use core:python@3.14`. To keep it for every project even when a plugin
is installed, add a [tool alias](/dev-tools/aliases.html):

```toml [mise.toml]
[tool_alias]
python = "core:python"
```
