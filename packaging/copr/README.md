# COPR Docker Build Environment

This directory contains a Docker-based environment for building and testing COPR packages locally.

## Overview

This setup allows you to:
- Build COPR source RPMs (SRPM) locally
- Test the build process without submitting to COPR
- Submit builds to COPR from a consistent environment

## Files

- `Dockerfile` - Docker image with COPR build dependencies (simplified for Docker workflow)
- `build-copr.sh` - Script for local development and testing
- `README.md` - This documentation

## Usage

### Building the Docker Image

```bash
cd packaging/copr
docker build -t mise-copr .
```

### Using the Build Script (Local Development)

The `build-copr.sh` script is designed for local development and testing. To use it:

```bash
# Copy the build script into the container
docker run --rm \
  -v $(pwd):/workspace \
  -v $(pwd)/packaging/copr/build-copr.sh:/usr/local/bin/build-copr.sh \
  mise-copr \
  /usr/local/bin/build-copr.sh \
  --version 2025.7.22 \
  --dry-run
```

### Running a Dry-Run Build

To build an SRPM locally without submitting to COPR:

```bash
# From the mise repository root
docker run --rm \
  -v $(pwd):/workspace \
  -v $(pwd)/packaging/copr/build-copr.sh:/usr/local/bin/build-copr.sh \
  mise-copr \
  /usr/local/bin/build-copr.sh \
  --version 2025.7.22 \
  --dry-run
```

### Submitting to COPR

To build and submit to COPR (requires API credentials):

```bash
# From the mise repository root
docker run --rm \
  -v $(pwd):/workspace \
  -v $(pwd)/packaging/copr/build-copr.sh:/usr/local/bin/build-copr.sh \
  -e COPR_API_LOGIN="your-login" \
  -e COPR_API_TOKEN="your-token" \
  mise-copr \
  /usr/local/bin/build-copr.sh \
  --version 2025.7.22 \
  --chroots "fedora-42-aarch64 fedora-42-x86_64"
```

### Script Options

The `build-copr.sh` script supports the following options:

- `-v, --version VERSION` - Package version (required)
- `-p, --profile PROFILE` - Build profile: `release` or `serious` (default: release)
- `-c, --chroots CHROOTS` - COPR build targets (default: fedora-42-aarch64 fedora-42-x86_64 epel-10-aarch64 epel-10-x86_64)
- `-o, --owner OWNER` - COPR owner (default: jdxcode)
- `-j, --project PROJECT` - COPR project name (default: mise)
- `-n, --name NAME` - Package name (default: mise)
- `-m, --maintainer-name NAME` - Maintainer name (default: mise Release Bot)
- `-e, --maintainer-email EMAIL` - Maintainer email (default: <noreply@mise.jdx.dev>)
- `-d, --dry-run` - Build SRPM only, don't submit to COPR
- `-h, --help` - Show help

### Environment Variables

For COPR submission, you need:
- `COPR_API_LOGIN` - Your COPR API login
- `COPR_API_TOKEN` - Your COPR API token

You can get these from your COPR account settings at: <https://copr.fedorainfracloud.org/api/>

## Examples

### Test Build with Different Profile

```bash
docker run --rm \
  -v $(pwd):/workspace \
  mise-copr \
  --version $(./scripts/get-version.sh | sed 's/^v//') \
  --profile serious \
  --dry-run
```

### Build for Specific Chroots

```bash
docker run --rm \
  -v $(pwd):/workspace \
  -e COPR_API_LOGIN="$COPR_API_LOGIN" \
  -e COPR_API_TOKEN="$COPR_API_TOKEN" \
  mise-copr \
  --version 2025.7.22 \
  --chroots "fedora-42-x86_64 epel-10-x86_64"
```

### Using Current Git Version

```bash
VERSION=$(./scripts/get-version.sh | sed 's/^v//')
docker run --rm \
  -v $(pwd):/workspace \
  mise-copr \
  --version "$VERSION" \
  --dry-run
```

## Artifacts

After running the build, artifacts will be available in the `artifacts/` directory:
- `*.src.rpm` - Source RPM package
- `*.spec` - RPM spec file used for the build

## Troubleshooting

### Build Failures

If the build fails:
1. Check the Docker logs for specific error messages
2. Examine the generated spec file in `artifacts/`
3. Verify that all submodules are properly initialized
4. Ensure the version format is correct (no 'v' prefix)

### COPR Submission Issues

If COPR submission fails:
1. Verify your API credentials are correct
2. Check that the COPR project exists and you have permissions
3. Ensure the chroots you specified are available in COPR

### Permission Issues

If you get permission errors with the mounted volume:
```bash
# On systems with SELinux, you might need:
docker run --rm \
  -v $(pwd):/workspace:Z \
  mise-copr \
  --version 2025.7.22 \
  --dry-run
```

## Development

The `build-copr.sh` script is derived from the GitHub Actions workflow `.github/workflows/copr-publish.yml`. When making changes to the build process, consider updating both files to maintain consistency.
