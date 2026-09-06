# URL Replacements

Use `url_replacements` to route requests made by mise's HTTP client through an internal
mirror or proxy. This can cover release metadata and artifact downloads, including Conda
channel metadata. It does not rewrite requests made independently by a plugin script,
Git, or an external package manager; configure those clients separately.

A replacement changes where a request is sent. It does not change which asset a backend
selects, create a mirror, or generate a new checksum. For Conda, lockfiles retain the logical
upstream URLs and the replacement is applied when the request is sent.

## Configuration Examples

Put machine-specific mirror settings in global configuration, or share them in `mise.toml`
when every user of the project has access to the mirror. For an exact URL prefix:

```toml
[settings]
url_replacements = { "https://example.com/" = "https://mirror.example.com/" }
```

The table form is convenient for several rules:

```toml
[settings.url_replacements]
"https://example.com/" = "https://mirror.example.com/"
"https://releases.hashicorp.com/" = "https://hashicorp.example.com/"
```

Replace the example mirror hosts with servers you operate or trust. They must serve the
paths and metadata expected by the original backend. Inspect `mise settings ls` to confirm
the settings in effect; use debug logging on the failing command to inspect its request URL.

## Simple Hostname Replacement

Despite often being used for hostnames, a plain key is a **substring match on the full URL**.
It can match a path or query string as well as a hostname. A key such as `github.com` also
matches `api.github.com` and `github.com.example.org`.

Including the scheme and trailing `/`, such as `https://github.com/`, avoids matching those
hostnames. For a rule that must match only at the start of a URL, use an anchored regex.

## Advanced Regex Replacement

Prefix a key with `regex:` to use the Rust regex engine. Capture groups in replacement
values use `$1`, `$2`, or named captures. The examples below use TOML literal strings for
regex keys, so backslashes do not need doubling.

### Regex Examples

#### 1. Protocol Conversion (HTTP to HTTPS)

```toml
[settings]
url_replacements = {
  'regex:^http://(.+)' = "https://$1",
}
```

Use this only when the destination supports HTTPS. A scheme change does not make an
untrusted server trustworthy.

#### 2. GitHub Release Mirroring with Path Restructuring

```toml
[settings]
url_replacements = {
  'regex:^https://github\.com/([^/]+)/([^/]+)/releases/download/(.+)' = "https://hub.example.com/artifactory/github/$1/$2/$3",
}
```

This maps `https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz` to
`https://hub.example.com/artifactory/github/owner/repo/v1.0.0/file.tar.gz`.
It does not rewrite `api.github.com` requests; add a separate rule if release metadata must
also pass through a mirror.

#### 3. Subdomain to Path Conversion

```toml
[settings]
url_replacements = {
  'regex:^https://([^./]+)\.cdn\.example\.com/(.+)' = "https://unified-cdn.example.com/$1/$2",
}
```

For example, `https://eu.cdn.example.com/tool.tar.gz` becomes
`https://unified-cdn.example.com/eu/tool.tar.gz`.

#### 4. Multiple Replacement Patterns (processed in order)

```toml
[settings]
url_replacements = {
  # Put the specific rule before the general GitHub rule.
  'regex:^https://github\.com/microsoft/(.+)' = "https://internal.example.org/microsoft/$1",
  'regex:^https://github\.com/(.+)' = "https://public.example.org/github/$1",
  "https://releases.hashicorp.com/" = "https://hashicorp.example.net/",
}
```

These examples use TOML 1.1 multiline inline tables, including comments and trailing commas.
The first rule handles Microsoft repositories, the second handles other GitHub paths, and
the last handles HashiCorp downloads.

## Regex Syntax

Use `^` to anchor the beginning, `(.+)` for a capture, and `[^/]+` for a path component.
Escape a literal dot with `\.` in a TOML literal string. In a double-quoted TOML string,
write `\\.` instead because TOML also processes backslash escapes.

The [Rust regex documentation](https://docs.rs/regex/latest/regex/#syntax) describes the
supported syntax. Backreferences inside the pattern and lookaround are not supported.
When a capture is followed by letters or digits, use braces to separate its name, such as
`${1}suffix`.

## Precedence and Matching

Rules run in configuration insertion order. mise uses the first rule that changes the URL
into another valid URL, then stops; replacements do not chain. Put specific rules before
broad ones. If a matching rule leaves the URL unchanged or produces an invalid URL, mise
continues to later rules. Invalid regex patterns produce a warning and are skipped.

If no rule produces a valid changed URL, the original request is used. URL replacements are
therefore routing rules, not an outbound-host allow list. Use network policy outside mise
when traffic must never reach an upstream host.

## Security Considerations

Authentication headers prepared for the original URL can be sent to the replacement server.
Only route requests to servers trusted to receive both the artifacts and those credentials.
When an HTTPS-to-HTTP rewrite would send an authorization header, cookie, API key, other recognized
credential header, or URL credentials, mise refuses the request rather than exposing them without
transport encryption. An unauthenticated downgrade is still allowed.
A host-changing rewrite removes credentials scoped to the original host, then can add mirror
credentials from netrc as described below. Same-host rewrites retain the existing credentials.

Use both a start anchor and a hostname boundary for an exact host:

```toml
[settings.url_replacements]
'regex:^https://github\.com/' = "https://mirror.example.com/"
```

The trailing slash matters: `^https://github\.com` by itself also matches
`https://github.com.example.org/`. Avoid placing credentials directly in replacement URLs,
which can appear in logs.

## Authentication

mise looks up netrc credentials **after** rewriting the URL. Use the replacement hostname
in `~/.netrc`, or `~/_netrc` on Windows (`~/.netrc` is a fallback there). The
[`netrc_file`](/configuration/settings.html#netrc_file) setting can select another file.

```netrc
machine mirror.example.com
  login myusername
  password mypassword
```

Use your mirror credentials and restrict the file's permissions on Unix, for example
`chmod 600 ~/.netrc`.

Netrc is normally a fallback: an existing Authorization header takes precedence. When a
replacement changes the hostname, matching netrc credentials for the new host can override
that header. A rewrite that only changes a path or query on the same host keeps existing
Authorization. See [GitHub Tokens](/dev-tools/github-tokens.html) for upstream token sources.
