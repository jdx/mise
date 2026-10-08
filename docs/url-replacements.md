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
for GitHub release and attestation metadata. If the proxy mirrors only release
downloads, keep only the first rule.

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

When a rule sends a request to a different host, mise drops the credentials
meant for the original host: the `Authorization` header, cookies, API key
headers, and any `user:password` in the URL. It then adds
[netrc](#netrc) credentials for the new host, if there are any. A rule that
keeps the host keeps the credentials.

mise refuses to send credentials over plain HTTP after an HTTPS-to-HTTP rewrite,
whether they came from the original request or are written into the rule. A
downgrade without credentials is allowed, and so is one whose credentials come
from a netrc entry for the new host. These rules also cover GitHub attestation
requests and Conda channels. For Sigstore TUF metadata, mise ignores such a
rewrite and keeps the default HTTPS URL.

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
a rule changes the host, netrc credentials for the new host replace that header.
A rule that changes only the path or query keeps the existing `Authorization`.
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
