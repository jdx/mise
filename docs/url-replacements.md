# URL Replacements

mise does not include a built-in registry for downloading artifacts.
Instead, it retrieves remote registry manifests, which specify the URLs for downloading tools.

In some environments — such as enterprises or DMZs — these URLs may not be directly accessible and must be accessed through a proxy or internal mirror.

URL replacements allow you to modify or redirect any URL that mise attempts to access, making it possible to use internal proxies, mirrors, or alternative sources as needed.

## Configuration Examples

In mise.toml (single line):

```toml
[settings]
url_replacements = { "example.com" = "mirror.example.com" }
```

In mise.toml (multiline):

```toml
[settings.url_replacements]
"example.com" = "mirror.example.com"
"releases.hashicorp.com" = "hashicorp.example.com"
```

RegEx example:

```toml
[settings.url_replacements]
"regex:^http://(.+)" = "https://$1"
"regex:^https://github\\.com/([^/]+)/([^/]+)/releases/download/(.+)" = "https://hub.example.com/artifactory/github/$1/$2/$3"
```

## Simple Hostname Replacement

For simple hostname-based mirroring, the key is the original hostname/domain to replace,
and the value is the replacement string. The replacement happens by searching and replacing
the pattern anywhere in the full URL string (including protocol, hostname, path, and query parameters).

Examples:

- `github.com` -> `mirror.example.com` replaces GitHub hostnames
- `https://github.com` -> `https://mirror.example.com` with protocol excludes e.g. 'api.github.com'
- `https://github.com` -> `https://proxy.example.com/github-mirror` replaces GitHub with corporate proxy
- `http://example.net` -> `https://example.net` replaces protocol from HTTP to HTTPS

See [Security Considerations](#security-considerations) for important warnings about credential handling.

## Advanced Regex Replacement

For more complex URL transformations, you can use regex patterns. When a key starts with `regex:`,
it is treated as a regular expression pattern that can match and transform any part of the URL.
The value can use capture groups from the regex pattern.

### Regex Examples

#### 1. Protocol Conversion (HTTP to HTTPS)

```toml
[settings]
url_replacements = {
  "regex:^http://(.+)" = "https://$1"
}
```

This converts any HTTP URL to HTTPS by capturing everything after "http://" and replacing it with "https://".

#### 2. GitHub Release Mirroring with Path Restructuring

```toml
[settings]
url_replacements = {
  "regex:^https://github\\.com/([^/]+)/([^/]+)/releases/download/(.+)" =
    "https://hub.example.com/artifactory/github/$1/$2/$3"
}
```

Transforms `https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz`
to `https://hub.example.com/artifactory/github/owner/repo/v1.0.0/file.tar.gz`

#### 3. Subdomain to Path Conversion

```toml
[settings]
url_replacements = {
  "regex:^https://([^.]+)\\.cdn\\.example\\.com/(.+)" =
    "https://unified-cdn.example.com/$1/$2"
}
```

Converts subdomain-based URLs to path-based URLs on a unified CDN.

#### 4. Multiple Replacement Patterns (processed in order)

```toml
[settings]
url_replacements = {
  "regex:^https://github\\.com/microsoft/(.+)" =
    "https://internal.example.org/microsoft/$1",
  "regex:^https://github\\.com/(.+)" =
    "https://public.example.org/github/$1",
  "releases.hashicorp.com" = "hashicorp.example.net"
}
```

First regex catches Microsoft repositories specifically, second catches all other GitHub URLs,
and the simple replacement handles HashiCorp.

## Use Cases

1. **Corporate Mirrors**: Replace public download URLs with internal corporate mirrors
2. **Custom Registries**: Redirect package downloads to custom or private registries
3. **Geographic Optimization**: Route downloads to geographically closer mirrors
4. **Protocol Changes**: Convert HTTP URLs to HTTPS or vice versa

## Regex Syntax

mise uses Rust regex engine which supports:

- `^` and `$` for anchors (start/end of string)
- `(.+)` for capture groups (use `$1`, `$2`, etc. in replacement)
- `[^/]+` for character classes (matches any character except `/`)
- `\\.` for escaping special characters (note: double backslash required in TOML)
- `*`, `+`, `?` for quantifiers
- `|` for alternation

You can check on regex101.com if your regex works (see [example](https://regex101.com/r/rmcIE1/1)).
Full regex syntax documentation: <https://docs.rs/regex/latest/regex/#syntax>

## Precedence and Matching

- URL replacements are processed in the order they appear in the configuration (IndexMap insertion order)
- Both regex patterns (keys starting with `regex:`) and simple string replacements are processed in this same order
- The first matching pattern is used; subsequent patterns are ignored for that URL
- If no patterns match, the original URL is used unchanged

## Security Considerations

When using regex patterns, ensure your replacement URLs point to trusted sources,
as this feature can redirect tool downloads to arbitrary locations.

> [!WARNING]
> **Credential Leaking**: When using `url_replacements`, any authentication headers (like `Authorization: Bearer <TOKEN>`) generated for the original URL (e.g., `api.github.com`) are **preserved** and sent to the replaced URL.
>
> This is by design to allow authentication with internal proxies that forward requests to upstream services (GitHub, GitLab, Forgejo, etc.). However, it means you must **only** replace URLs with trusted servers. Redirecting to an untrusted server will leak your credentials to that server.
>
> **Best Practice**: Use the `^` anchor in your regex patterns to ensure you are matching the start of the URL.
>
> **Bad**: `"regex:github\\.com"` (matches `evil-github.com`)
> **Good**: `"regex:^https://github\\.com"` (only matches actual GitHub URLs)

## Authentication

Can be used with `~/.netrc` (or `~/_netrc` on Windows) to authenticate with the replaced URL.
Replacements are applied _before_ the netrc lookup, so you should use the hostname of the _replaced_ URL in your netrc file.

For example, if you have this in `mise.toml`:

```toml
[settings]
url_replacements = { "regex:^https://github\\.com" = "https://nexus.example.com" }
```

> [!NOTE]
> Credentials from `.netrc` take precedence over and will **overwrite** any default authentication headers (such as those from `MISE_GITHUB_TOKEN` or other environment variables).

You should have this in `~/.netrc`:

```netrc
machine nexus.example.com
  login myusername
  password mypassword
```
