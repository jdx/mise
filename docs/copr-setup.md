# COPR Publishing Setup

This document explains how to set up and use the GitHub Actions workflow for publishing mise to Fedora COPR (Community Projects) repositories.

## Prerequisites

1. **Fedora Account**: You need a Fedora account with COPR access
2. **COPR API Token**: API credentials for automated submissions
3. **GitHub Repository Secrets**: Required secrets configured in GitHub

## Setting up COPR Project

1. Create a Fedora account at <https://accounts.fedoraproject.org>
2. Access COPR at <https://copr.fedorainfracloud.org>
3. Create a new project for mise (e.g., `jdx/mise`)
4. Configure the project settings:
   - Enable the desired chroots (Fedora versions, EPEL, etc.)
   - Set build timeout and other preferences
   - Configure automatic builds if desired

## COPR API Setup

Generate API credentials for automated submissions:

1. Go to <https://copr.fedorainfracloud.org/api/>
2. Generate a new API token
3. Note down your login, username, and token

## GitHub Repository Configuration

### Required Secrets

Configure the following secrets in your GitHub repository settings:

- `COPR_API_LOGIN`: Your COPR API login
- `COPR_API_TOKEN`: Your COPR API token

### Repository Variables

Configure these variables in your GitHub repository settings:

- `COPR_MAINTAINER_NAME`: Your name (e.g., "John Doe")
- `COPR_MAINTAINER_EMAIL`: Your email address
- `COPR_OWNER`: Your COPR username (e.g., "jdx")
- `COPR_PROJECT`: Your COPR project name (e.g., "mise")

### Environment Protection

The workflow uses a `copr-publishing` environment for additional security. Configure this in your repository settings under Environments.

## How the Workflow Works

The workflow follows RPM packaging best practices and:

1. **Triggers**: Runs on new GitHub releases or manual workflow dispatch
2. **Versioning**: Uses `scripts/get-version.sh` to get the current version (same as other workflows)
3. **Submodules**: Includes the `aqua-registry` submodule in the source package
4. **Vendoring**: Uses `cargo vendor` to bundle all Rust dependencies
5. **RPM Packaging**: Creates proper RPM source packages with:
   - `.spec` file with build instructions
   - Source tarball with vendored dependencies
   - Proper metadata and dependencies
6. **Multi-distribution**: Builds packages for multiple Fedora/EPEL versions
7. **COPR Submission**: Submits source RPM to COPR using `copr-cli`
8. **Build Monitoring**: COPR automatically builds binary packages for all configured chroots

## Usage

### Automatic Release Publishing

The workflow automatically triggers when you create a new GitHub release:

1. Create a new release on GitHub
2. The workflow will automatically package and submit to COPR
3. Monitor the Actions tab for progress
4. Check your COPR project for build status

### Manual Publishing

You can manually trigger the workflow via GitHub Actions:

1. Go to the Actions tab in your repository
2. Select "Publish to COPR" workflow
3. Click "Run workflow"
4. Specify the target chroots (optional)
5. Optionally enable the "serious" profile for optimized builds with LTO

### Supported Distributions

By default, the workflow targets:
- Fedora 39 (x86_64)
- Fedora 40 (x86_64)  
- Fedora 41 (x86_64)
- EPEL 9 (x86_64) - for RHEL 9, AlmaLinux 9, Rocky Linux 9

You can customize this by modifying the `chroots` input when manually running the workflow.

## Package Installation

Once published and built, users can install your package:

### Fedora
```bash
sudo dnf copr enable jdx/mise
sudo dnf install mise
```

### RHEL/AlmaLinux/Rocky Linux (with EPEL)
```bash
sudo dnf install epel-release
sudo dnf copr enable jdx/mise
sudo dnf install mise
```

### CentOS Stream
```bash
sudo dnf install epel-release
sudo dnf copr enable jdx/mise
sudo dnf install mise
```

## Troubleshooting

### Common Issues

1. **API Authentication Failures**:
   - Ensure your COPR API login and token are correctly stored in GitHub Secrets
   - Verify the API credentials work by testing with `copr-cli` locally

2. **Build Failures**:
   - Check COPR build logs for specific error messages
   - Verify all Rust dependencies can be vendored
   - Ensure the project builds successfully with `cargo build --release`

3. **Spec File Issues**:
   - Review the generated `.spec` file in the workflow artifacts
   - Check that all required BuildRequires are specified
   - Verify file paths and permissions are correct

4. **Chroot Compatibility**:
   - Some packages may not build on all target distributions
   - Adjust the `chroots` list to exclude problematic targets
   - Consider distribution-specific spec file modifications

### Debugging

The workflow creates artifacts containing:
- Generated source RPM (`.src.rpm`)
- Generated spec file (`.spec`)

Download these artifacts to inspect the packaging if issues occur.

### Build Status Monitoring

Monitor build status at:
- Your COPR project page: `https://copr.fedorainfracloud.org/coprs/[owner]/[project]/`
- Build details and logs are available for each chroot
- Failed builds include detailed error logs

## Security Considerations

- Store API credentials securely in GitHub Secrets
- Use environment protection rules for the `copr-publishing` environment
- Regularly rotate API tokens and update secrets accordingly
- Monitor COPR submissions for unauthorized changes
- Consider enabling COPR project permissions and collaboration controls

## Customization

To customize the packaging:

1. **Modify Dependencies**: Update the `BuildRequires` section in the generated spec file
2. **Add Custom Files**: Extend the `%files` section for additional files
3. **Build Options**: Modify the `%build` section for custom build flags
4. **Post-install Scripts**: Add `%post`, `%preun`, etc. sections as needed
5. **Multiple Packages**: Split into subpackages if needed (e.g., `-devel`, `-doc`)

### Advanced Spec File Customization

For more complex packaging needs, you can:

1. Create a custom spec file template in the repository
2. Modify the workflow to use your template instead of the generated one
3. Use conditional builds for different distributions
4. Add systemd service files or other distribution-specific features

## COPR vs Traditional RPM Repositories

COPR provides several advantages:

- **Easy Setup**: No need to maintain your own repository infrastructure
- **Automatic Building**: Builds packages for multiple distributions automatically
- **Community Trust**: Uses Fedora infrastructure with established security practices
- **Integration**: Easy integration with DNF/YUM package managers
- **Build Logs**: Comprehensive build logging and debugging information

However, consider traditional repositories for:
- Enterprise environments requiring more control
- Packages requiring special build environments
- High-availability requirements beyond COPR's SLA

## Migration from Other Packaging Systems

If migrating from other packaging systems:

1. **From PPA**: The workflow structure is similar, but RPM packaging differs significantly
2. **From AUR**: COPR provides binary packages vs source-based AUR
3. **From Custom RPM**: Migrate existing spec files and adapt for COPR environment

The workflow is designed to complement the existing PPA workflow, allowing dual publication to both Ubuntu PPAs and Fedora COPR repositories.
