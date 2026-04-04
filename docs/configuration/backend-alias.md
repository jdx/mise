# Backend Aliases

Backend aliases let you define a named shorthand for a backend with preset default options. This is useful when you frequently install tools from the same backend instance and want to avoid repeating shared configuration on every tool entry.

The most common use case is a self-hosted GitLab or GitHub instance where you always need to specify `api_url`:

```toml [mise.toml]
[backend_alias]
mygitlab = { backend = "gitlab", api_url = "https://gitlab.mycompany.com/api/v4" }

[tools]
"mygitlab:mycompany/tool1" = "latest"
"mygitlab:mycompany/tool2" = "1.0"
"mygitlab:mycompany/tool3" = { version = "2.0", asset_pattern = "tool3-linux-x64" }
```

Without backend aliases, you would have to repeat `api_url` for every tool.

## Syntax

Define backend aliases in a `[backend_alias]` section. Each entry maps an alias name to a backend definition:

```toml
[backend_alias]
<alias-name> = { backend = "<backend>", <option> = <value>, ... }
```

Or using a table:

```toml
[backend_alias.<alias-name>]
backend = "<backend>"
<option> = <value>
```

The `backend` key is required and must be a valid mise backend (e.g., `github`, `gitlab`, `cargo`, `npm`). All other keys become default tool options for tools that use this alias.

## Option Priority

When a tool is installed via a backend alias, options are merged with the following priority (highest wins):

1. **Per-tool options** — options specified directly on the tool entry
2. **Alias defaults** — options from the `[backend_alias]` definition
3. **Registry defaults** — options from the mise registry

For example:

```toml
[backend_alias]
mygitlab = { backend = "gitlab", api_url = "https://gitlab.mycompany.com/api/v4", asset_pattern = "tool-linux-x64" }

[tools]
# Uses alias api_url and asset_pattern
"mygitlab:mycompany/tool1" = "latest"

# Uses alias api_url, but overrides asset_pattern
"mygitlab:mycompany/tool2" = { version = "latest", asset_pattern = "tool2-linux-x86_64" }
```

## Configuration Layering

Backend aliases follow the standard [configuration hierarchy](/configuration#configuration-hierarchy). Aliases defined in a more specific config (closer to your project) override aliases with the same name in broader configs (global, system).

A common pattern is to define shared aliases globally and use them in project configs:

```toml [~/.config/mise/config.toml]
[backend_alias]
mygitlab = { backend = "gitlab", api_url = "https://gitlab.mycompany.com/api/v4" }
```

```toml [~/work/myproject/mise.toml]
[tools]
"mygitlab:mycompany/tool1" = "latest"
"mygitlab:mycompany/tool2" = "1.0"
```

## Authentication

All authentication mechanisms for the underlying backend apply unchanged. For example, tokens and environment variables work the same way whether you use the full backend name or an alias:

```sh
export MISE_GITLAB_ENTERPRISE_TOKEN="your-token"
```

See the documentation for each backend for details:

- [GitHub backend](/dev-tools/backends/github)
- [GitLab backend](/dev-tools/backends/gitlab)
