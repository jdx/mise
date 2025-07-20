# PPA Publishing Setup

This document explains how to set up and use the GitHub Actions workflow for publishing mise to Ubuntu PPAs (Personal Package Archives).

## Prerequisites

1. **Launchpad Account**: You need a Launchpad account with PPA access
2. **GPG Key**: A GPG key for signing packages
3. **GitHub Repository Secrets**: Required secrets configured in GitHub

## Setting up Launchpad and PPA

1. Create a Launchpad account at https://launchpad.net
2. Create a new PPA for your project (e.g., `ppa:jdxcode/mise`)
3. Generate and upload your GPG public key to Launchpad

## GPG Key Setup

Generate a GPG key if you don't have one:

```bash
gpg --gen-key
```

Export your private key:
```bash
gpg --armor --export-secret-keys YOUR_EMAIL > private-key.asc
```

Export your public key and upload it to Launchpad:
```bash
gpg --armor --export YOUR_EMAIL > public-key.asc
```

## GitHub Repository Configuration

### Required Secrets

Configure the following secrets in your GitHub repository settings:

- `MISE_GPG_KEY`: Your GPG private key (entire content of private-key.asc)
  - This follows the same pattern as other mise workflows

### Repository Variables

Configure these variables in your GitHub repository settings:

- `PPA_MAINTAINER_NAME`: Your name (e.g., "John Doe")
- `PPA_MAINTAINER_EMAIL`: Your email address (must match GPG key)
- `PPA_NAME`: Your PPA identifier (e.g., "ppa:jdxcode/mise")

### Environment Protection

The workflow uses a `ppa-publishing` environment for additional security. Configure this in your repository settings under Environments.

## How the Workflow Works

The workflow follows the Rust PPA packaging guide and:

1. **Triggers**: Runs on new GitHub releases or manual workflow dispatch
2. **Versioning**: Uses `scripts/get-version.sh` to get the current version (same as other workflows)
3. **Submodules**: Includes the `aqua-registry` submodule in the source package
4. **Vendoring**: Uses `cargo vendor` to bundle all dependencies
5. **Debian Packaging**: Creates proper Debian source packages with:
   - `debian/control` - Package metadata
   - `debian/rules` - Build instructions
   - `debian/changelog` - Version history
   - `debian/copyright` - License information
6. **Multi-distribution**: Builds packages for multiple Ubuntu versions (focal, jammy, noble)
7. **Signing**: Signs packages with your GPG key using the same pattern as other workflows
8. **Upload**: Uploads source packages to your PPA using `dput`

## Usage

### Automatic Release Publishing

The workflow automatically triggers when you create a new GitHub release:

1. Create a new release on GitHub
2. The workflow will automatically package and upload to your PPA
3. Monitor the Actions tab for progress

### Manual Publishing

You can manually trigger the workflow via GitHub Actions:

1. Go to the Actions tab in your repository
2. Select "Publish to PPA" workflow
3. Click "Run workflow"
4. Specify the version and target distributions

### Supported Ubuntu Distributions

By default, the workflow targets:
- Ubuntu 20.04 LTS (focal)
- Ubuntu 22.04 LTS (jammy)
- Ubuntu 24.04 LTS (noble)

You can customize this by modifying the `distributions` input when manually running the workflow.

## Package Installation

Once published, users can install your package:

```bash
sudo add-apt-repository ppa:jdxcode/mise
sudo apt update
sudo apt install mise
```

## Troubleshooting

### Common Issues

1. **GPG Signing Failures**: 
   - Ensure your GPG private key is correctly stored in the `MISE_GPG_KEY` secret
   - The workflow uses the same GPG setup as other mise workflows

2. **Build Failures**:
   - Verify all Rust dependencies can be vendored
   - Check that the project builds successfully with `cargo build --release`

3. **Upload Failures**:
   - Confirm your PPA name is correct in variables
   - Ensure your GPG public key is uploaded to Launchpad

### Debugging

The workflow creates artifacts containing the generated source packages. Download these to inspect the generated Debian files if issues occur.

## Security Considerations

- Store GPG private keys securely in GitHub Secrets
- Use environment protection rules for the `ppa-publishing` environment
- Regularly rotate GPG keys and update secrets accordingly
- Monitor PPA uploads for unauthorized changes

## Customization

To customize the packaging:

1. Modify the `debian/control` generation in the workflow for different dependencies
2. Adjust the `debian/rules` section for custom build steps
3. Update the package description and metadata
4. Add additional files like systemd services or configuration files

The workflow is designed to be generic enough for most Rust CLI tools but can be adapted for specific needs.