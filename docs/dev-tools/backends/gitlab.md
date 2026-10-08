---
description: "Install prebuilt tools from GitLab release links on gitlab.com or a self-managed instance."
---

# gitlab backend

The `gitlab` backend installs a tool from the files linked in a GitLab project's
releases, on gitlab.com or a self-managed instance. It shares its implementation
with the [github backend](/dev-tools/backends/github.html) and accepts the same
[tool options](/dev-tools/backends/github.html#tool-options); this page covers
what is different.

## Usage

List a project's versions:

```sh
mise ls-remote gitlab:gitlab-org/gitlab-runner
```

GitLab Runner names its release links after the platform, such as
`binary: Linux amd64`, not after the file. This configuration matches each
platform's link by that name and names the downloaded executable:

```toml
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "19.3.1"
bin = "gitlab-runner"

[tools."gitlab:gitlab-org/gitlab-runner".platforms]
linux-x64 = { asset_pattern = "binary: Linux amd64" }
macos-arm64 = { asset_pattern = "binary: macOS arm64" }
```

Install it and run it:

```sh
mise install
mise exec -- gitlab-runner --version
```

This installs the executable; it does not register a runner or start a service.
For another platform, look up the link names on the project's Releases page and
add a `platforms` entry.

## Differences from the github backend

| Behavior                                                                       | github                                                       | gitlab                                                                                     |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| Name tested by `asset_pattern`, `matching`, `matching_regex` and autodetection | The asset's file name                                        | The release link's display name, such as `binary: Linux amd64`                             |
| Releases that `mise ls-remote` lists                                           | Releases with assets, from up to three pages of 100          | Releases with at least one link, from the newest 100 only                                  |
| `latest`                                                                       | The release GitHub marks as Latest                           | The newest listed release                                                                  |
| `prerelease`                                                                   | Uses GitHub's prerelease flag                                | GitLab has no prerelease flag, so every release is listed; see [Prereleases](#prereleases) |
| Checksums found automatically                                                  | The digest GitHub reports, or a checksum file in the release | A checksum file among the release links                                                    |
| GitHub artifact attestations and SLSA provenance                               | Checked                                                      | Not available; pin artifacts with `checksum` or [mise.lock](/dev-tools/mise-lock.html)     |
| Default `api_url`                                                              | `https://api.github.com`                                     | `https://gitlab.com/api/v4`                                                                |

### Link names

Because mise matches release links by display name, write `asset_pattern` and
`matching` against the names shown on the Releases page. mise names the
downloaded file after the link too, so when a link's name has no file
extension, set [`bin`](/dev-tools/backends/github.html#bin) for a single
binary, as in the example above, or
[`format`](/dev-tools/backends/github.html#format) for an archive.

### Version listing

mise reads only the newest 100 releases, so `mise ls-remote` and version
prefixes do not see older ones. Set `MISE_LIST_ALL_VERSIONS=1` to list every
release. An exact version, such as `@16.8.0`, installs even when it is not
listed. mise lists only releases that have at least one link, never Git tags or
GitLab's generated source archives.

### Prereleases

GitLab releases carry no prerelease flag, so `mise ls-remote` lists every
release. `latest` and version prefixes still skip versions whose names look like
prereleases, such as `2.0.0-rc1`. Set
[`prerelease = true`](/dev-tools/backends/github.html#prerelease) on the tool to
include them.

## Authentication

Public projects need no token. For a private project, a self-managed instance
or rate limits, set `MISE_GITLAB_TOKEN` or another token source; see
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html). Run
`mise token gitlab` to see which token mise uses.

## Self-managed GitLab

Set `api_url` to your instance's API and provide a token for that host, for
example with `MISE_GITLAB_ENTERPRISE_TOKEN`; see
[self-managed hosts](/dev-tools/github-tokens.html#github-enterprise). mise
lists releases through that API and downloads each release link from the URL it
names.

### `api_url` {#api-url}

The base URL of the GitLab API. It defaults to `https://gitlab.com/api/v4`.

```toml
[tools]
"gitlab:myorg/mytool" = { version = "latest", api_url = "https://gitlab.mycompany.com/api/v4" }
```

## Tool options

The `gitlab` backend accepts the
[github backend's tool options](/dev-tools/backends/github.html#tool-options)
for selecting assets, choosing versions, extracting files and verifying
downloads, with the differences listed above. The GitHub-only options
`github_attestations`, `slsa_signer_identity` and `slsa_signer_issuer` have no
effect here.

## Settings

The [`gitlab` settings](/configuration/settings.html#gitlab) choose the token
mise sends; see
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html).
