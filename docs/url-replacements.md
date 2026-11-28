---
description: "Rewrite the URLs mise requests with url_replacements to send downloads and release metadata through a mirror or proxy."
socialDescription: "Rewrite the URLs mise requests to send downloads and metadata through a mirror or proxy."
---

# URL replacements

The [`url_replacements`](/configuration/settings.html#url_replacements) setting
rewrites the URLs that mise's HTTP client requests, so release metadata and
artifact downloads can go through an internal mirror or proxy. It also covers
Conda channel metadata and the HTTP requests that vfox plugins make through
their `http` module. It does not rewrite requests made by asdf plugin scripts,
commands a plugin runs (such as `curl`), Git, or an external package manager;
configure those clients separately.

A replacement changes where a request is sent. It does not change which asset a
backend selects, create a mirror, or produce new checksums. For Conda,
lockfiles keep the upstream URLs, and the replacement applies when the request
is sent.

```toml [~/.config/mise/config.toml]
[settings.url_replacements]
"https://releases.hashicorp.com/" = "https://hashicorp-mirror.example.com/"
```

Put machine-specific mirrors in global config, or in `mise.toml` when everyone
on the project can reach the mirror. Replace the example hosts with servers you
operate or trust; they must serve the same paths and metadata as the original.

## Route GitHub through a proxy

A plain key keeps the rest of the URL, so a proxy that mirrors GitHub under a
path prefix needs no regular expression:

```toml
[settings.url_replacements]
"https://github.com/" = "http://pkgproxy.internal:8080/generic/github/"
"https://api.github.com/" = "http://pkgproxy.internal:8080/generic/github-api/"
```

| Original request                                                       | Sent to                                                                                  |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz`   | `http://pkgproxy.internal:8080/generic/github/owner/repo/releases/download/v1.0.0/file.tar.gz` |
| `https://api.github.com/repos/owner/repo/releases`                     | `http://pkgproxy.internal:8080/generic/github-api/repos/owner/repo/releases`             |

`https://github.com/` does not match `https://api.github.com/`, so the two rules
do not overlap. Reroute the API only if the proxy serves it: doing so also stops
mise from using [mise-versions](/configuration/settings.html#use_versions_host)
for GitHub release and attestation metadata. A rerouted API also cannot confirm
a repository rename or transfer, so GitHub attestations and SLSA provenance have
to name the repository exactly as you configured it. If the proxy mirrors only
release downloads, keep only the first rule.

## How rules match

A plain key matches anywhere in the URL, including the path and query, and every
occurrence is replaced. `github.com` also matches `api.github.com` and
`github.com.example.org`. Include the scheme and a trailing slash, such as
`https://github.com/`, to match one host.

A key that starts with `regex:` is a regular expression; see
[Regex rules](#regex-rules).

Rules run in the order they appear. mise uses the first rule that turns the URL
into a different valid URL, then stops, so replacements do not chain. A rule
that leaves the URL unchanged or produces an invalid URL is skipped, and an
invalid regular expression produces a warning and is skipped. When no rule
changes the URL, mise sends the original request. Put specific rules before
broad ones.

URL replacements are routing rules, not an allow list. Use network policy
outside mise when traffic must never reach an upstream host.

## Regex rules

The examples below use TOML literal strings (single quotes) for regex keys, so
backslashes do not need doubling. In a double-quoted TOML string, write `\\.`
instead of `\.`.

Use `^` to anchor the start, `(.+)` to capture, and `[^/]+` for one path
component. Refer to captures in the replacement as `$1`, `$2`, or by name. When
a capture is followed by letters or digits, add braces, as in `${1}suffix`. The
[Rust regex documentation](https://docs.rs/regex/latest/regex/#syntax) describes
the syntax; backreferences inside the pattern and lookaround are not supported.

To match exactly one host, anchor the start and end the host with a slash:

```toml
[settings.url_replacements]
'regex:^https://github\.com/' = "https://mirror.example.com/"
```

Without the trailing slash, `^https://github\.com` also matches
`https://github.com.example.org/`.

### Restructure GitHub release paths

This rule sends `https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz`
to `https://hub.example.com/artifactory/github/owner/repo/v1.0.0/file.tar.gz`:

```toml
[settings.url_replacements]
'regex:^https://github\.com/([^/]+)/([^/]+)/releases/download/(.+)' = "https://hub.example.com/artifactory/github/$1/$2/$3"
```

It does not rewrite `api.github.com` requests; add a separate rule if release
metadata must also go through the mirror.

### Subdomain to path

This rule sends `https://eu.cdn.example.com/tool.tar.gz` to
`https://unified-cdn.example.com/eu/tool.tar.gz`:

```toml
[settings.url_replacements]
'regex:^https://([^./]+)\.cdn\.example\.com/(.+)' = "https://unified-cdn.example.com/$1/$2"
```

### Specific rule before a general one

The first rule handles Microsoft repositories, the second handles every other
GitHub path, and the third handles HashiCorp downloads:

```toml
[settings.url_replacements]
'regex:^https://github\.com/microsoft/(.+)' = "https://internal.example.org/microsoft/$1"
'regex:^https://github\.com/(.+)' = "https://public.example.org/github/$1"
"https://releases.hashicorp.com/" = "https://hashicorp.example.net/"
```

### HTTP to HTTPS

```toml
[settings.url_replacements]
'regex:^http://(.+)' = "https://$1"
```

Use this only when the destination supports HTTPS. Changing the scheme does not
make an untrusted server trustworthy.

## Credentials

When a rule sends a request to another server (a different host, or the same
host on another port), mise drops the credentials meant for the original URL:
the `Authorization`, `Proxy-Authorization` and `Cookie` headers, headers whose
name marks them as an API key, token, secret or credential, and the
`user:password` the original URL carried. For a different host it then adds
[netrc](#netrc) credentials for the new host, if there are any; for another
port of the same host it adds none. A rule that keeps the host and port keeps
the credentials, including one that only switches between HTTP and HTTPS on
their default ports.

An HTTPS-to-HTTP rewrite is refused only when the request would still carry
one of those credentials over plain HTTP:

- the original request's credentials, when the rule keeps the host and port;
- a `user:password` written into the rule itself, whatever host it names;
- a netrc login for the original host, when the rule keeps the host and port.

Any other downgrade goes ahead. A rule to another port of the original host is
sent without those credentials, and one to another host with that host's netrc
login, if it has one. These rules also cover Conda channels.
For GitHub attestation requests, mise drops the GitHub token when the rule
changes the host, and refuses an HTTPS-to-HTTP rewrite that keeps the host. For
Sigstore TUF metadata, mise ignores such a rewrite and keeps the default HTTPS
URL.

::: warning
mise does not drop or check the query string. A rewrite keeps it as it is, so
a token in the query, such as the signature of a signed URL, goes to the new
server, and over plain HTTP after a downgrade. Don't point a signed URL, or any
URL with a secret in its query, at an `http://` replacement.
:::

Avoid writing credentials into replacement URLs, where they can appear in logs.
Only route requests to servers you trust with both the artifacts and the
credentials.

### netrc {#netrc}

mise looks up netrc credentials after it rewrites the URL, so use the
replacement host in `~/.netrc` (`~/_netrc` on Windows, with `~/.netrc` as a
fallback). The [`netrc_file`](/configuration/settings.html#netrc_file) setting
selects another file.

```netrc
machine mirror.example.com
  login myusername
  password mypassword
```

On Unix, restrict the file's permissions with `chmod 600 ~/.netrc`.

netrc credentials are a fallback: an existing `Authorization` header wins. When
a rule changes the host, that header is dropped and netrc credentials for the
new host take its place. A rule that moves to another port of the same host
drops the header and adds no netrc credentials, since a netrc entry names a
host, not the server on each of its ports. A rule that changes only the path or
query keeps the existing `Authorization`.
See [GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html) for
where upstream tokens come from.

## Set rules from the environment

`MISE_URL_REPLACEMENTS` takes the same rules as a JSON object, which suits a
single command or a CI job:

```sh
MISE_URL_REPLACEMENTS='{"https://github.com/":"https://mirror.example.com/github/"}' mise install
```

## Check a rule

Set `MISE_LOG_HTTP=1` to print each request mise sends, after rewriting, with its
response status. With the rule from the previous example, installing jq prints
lines such as:

```sh
export MISE_URL_REPLACEMENTS='{"https://github.com/":"https://mirror.example.com/github/"}'
MISE_LOG_HTTP=1 mise install jq@1.8.1
```

```text
GET https://mirror.example.com/github/jqlang/jq/releases/download/jq-1.8.1/jq-linux-amd64 200 OK
```

Version lists are cached, so run `mise cache clear <tool>` first when you test a
rule for metadata requests.
