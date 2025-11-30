# Netrc Authentication

The netrc file is a standard mechanism for storing credentials which allows tools to authenticate with online services, without user interaction. When mise makes HTTP requests to download tools, it will automatically look up credentials in your netrc file and apply HTTP Basic authentication for matching hosts.

## Configuration

### Enabling/Disabling Netrc

Netrc support is enabled by default. You can disable it via settings:

```toml
# mise.toml
[settings]
netrc = false
```

Or via environment variable:

```bash
export MISE_NETRC=false
```

### Custom Netrc File Path

By default, mise looks for the netrc file at:

- **Unix/macOS**: `~/.netrc`
- **Windows**: `%USERPROFILE%\_netrc` (falls back to `%USERPROFILE%\.netrc`)

You can also specify a custom path:

```toml
# mise.toml
[settings]
netrc_file = "/path/to/custom/netrc"
```

Or via environment variable:

```bash
export MISE_NETRC_FILE=/path/to/custom/netrc
```

## Netrc File Format

The netrc file uses a simple format with machine entries:

```
machine artifactory.example.com
  login myuser
  password mytoken

machine nexus.company.com
  login admin
  password secretpassword

default
  login anonymous
  password anonymous@example.com
```

### Keywords

- **machine**: Specifies the hostname to match (case-insensitive)
- **login**: The username for authentication
- **password**: The password or token for authentication
- **default**: Optional fallback credentials for any host not explicitly listed

### Inline Format

You can also use a single-line format:

```
machine example.com login myuser password mypassword
```

## Use Cases

## Credential Priority

When mise makes HTTP requests, netrc credentials are applied after [URL replacements](/url-replacements.md). This means:

1. **URL Replacement**: If configured, URLs are first transformed (e.g., `github.com` → `artifactory.mycompany.com`)
2. **Netrc Lookup**: Credentials are looked up based on the final (replaced) URL's host
3. **Header Override**: If netrc credentials are found for the host, they override any existing authorization headers (including GitHub/GitLab tokens)

## Security Considerations

### File Permissions

The netrc file contains sensitive credentials. Ensure proper file permissions:

```bash
chmod 600 ~/.netrc
```

## Troubleshooting

### Verify Netrc is Being Loaded

Enable debug logging to see if netrc is loaded:

```bash
MISE_DEBUG=1 mise install
```

Look for messages like:

```
DEBUG Loaded netrc from /home/user/.netrc
```

### Check Host Matching

Host matching is case-insensitive. Ensure your netrc entry matches the exact hostname in the URL:

```
# Matches: https://artifactory.example.com/...
machine artifactory.example.com
  login user
  password pass
```

### Netrc Not Found

If mise can't find your netrc file:

1. Verify the file exists at the expected path
2. Check file permissions (must be readable for the current user)

### Authentication Still Failing

Verify credentials are correct by testing manually:

```bash
curl --netrc-file /path/to/netrc-file https://your-host.com/path
```

## Related

- **[URL Replacements](/url-replacements.md)** - Redirect download URLs to internal mirrors
- **[Settings](/configuration/settings.md)** - All mise configuration options
