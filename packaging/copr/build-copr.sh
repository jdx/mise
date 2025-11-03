#!/bin/bash

set -euo pipefail

# Default values
PACKAGE_NAME="${PACKAGE_NAME:-mise}"
CHROOTS="${CHROOTS:-fedora-42-aarch64 fedora-42-x86_64 epel-10-aarch64 epel-10-x86_64}"
BUILD_PROFILE="${BUILD_PROFILE:-release}"
MAINTAINER_NAME="${MAINTAINER_NAME:-mise Release Bot}"
MAINTAINER_EMAIL="${MAINTAINER_EMAIL:-noreply@mise.jdx.dev}"
COPR_OWNER="${COPR_OWNER:-jdxcode}"
COPR_PROJECT="${COPR_PROJECT:-mise}"
DRY_RUN="${DRY_RUN:-false}"

# Store the repository root directory
REPO_ROOT="$(pwd)"

# Function to show usage
usage() {
	echo "Usage: $0 [options]"
	echo ""
	echo "Options:"
	echo "  -v, --version VERSION        Package version (required)"
	echo "  -p, --profile PROFILE        Build profile (default: release)"
	echo "  -c, --chroots CHROOTS        COPR chroots (default: fedora-42-aarch64 fedora-42-x86_64 epel-10-aarch64 epel-10-x86_64)"
	echo "  -o, --owner OWNER            COPR owner (default: jdxcode)"
	echo "  -j, --project PROJECT        COPR project (default: mise)"
	echo "  -n, --name NAME              Package name (default: mise)"
	echo "  -m, --maintainer-name NAME   Maintainer name (default: mise Release Bot)"
	echo "  -e, --maintainer-email EMAIL Maintainer email (default: noreply@mise.jdx.dev)"
	echo "  -d, --dry-run                Build SRPM only, don't submit to COPR"
	echo "  -h, --help                   Show this help"
	echo ""
	echo "Environment variables:"
	echo "  COPR_API_LOGIN               COPR API login (required for submission)"
	echo "  COPR_API_TOKEN               COPR API token (required for submission)"
	echo ""
	echo "Example:"
	echo "  $0 -v 2025.7.22 -p serious -d"
	exit 0
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
	case $1 in
	-v | --version)
		VERSION="$2"
		shift 2
		;;
	-p | --profile)
		BUILD_PROFILE="$2"
		shift 2
		;;
	-c | --chroots)
		CHROOTS="$2"
		shift 2
		;;
	-o | --owner)
		COPR_OWNER="$2"
		shift 2
		;;
	-j | --project)
		COPR_PROJECT="$2"
		shift 2
		;;
	-n | --name)
		PACKAGE_NAME="$2"
		shift 2
		;;
	-m | --maintainer-name)
		MAINTAINER_NAME="$2"
		shift 2
		;;
	-e | --maintainer-email)
		MAINTAINER_EMAIL="$2"
		shift 2
		;;
	-d | --dry-run)
		DRY_RUN="true"
		shift
		;;
	-h | --help)
		usage
		;;
	*)
		echo "Unknown option: $1"
		usage
		;;
	esac
done

# Check required parameters
if [ -z "${VERSION:-}" ]; then
	echo "Error: VERSION is required"
	echo "Use --version to specify the version or set VERSION environment variable"
	exit 1
fi

echo "=== COPR Build Configuration ==="
echo "Package Name: $PACKAGE_NAME"
echo "Version: $VERSION"
echo "Build Profile: $BUILD_PROFILE"
echo "Chroots: $CHROOTS"
echo "COPR Owner: $COPR_OWNER"
echo "COPR Project: $COPR_PROJECT"
echo "Maintainer: $MAINTAINER_NAME <$MAINTAINER_EMAIL>"
echo "Dry Run: $DRY_RUN"
echo ""

# Configure Git (needed for git archive)
git config --global user.name "$MAINTAINER_NAME"
git config --global user.email "$MAINTAINER_EMAIL"
git config --global --add safe.directory "$REPO_ROOT"

# Set up COPR configuration if not in dry-run mode
if [ "$DRY_RUN" != "true" ]; then
	if [ -z "${COPR_API_LOGIN:-}" ] || [ -z "${COPR_API_TOKEN:-}" ]; then
		echo "Error: COPR_API_LOGIN and COPR_API_TOKEN environment variables are required for submission"
		exit 1
	fi

	mkdir -p ~/.config
	cat >~/.config/copr <<EOF
[copr-cli]
login = $COPR_API_LOGIN
username = $COPR_OWNER
token = $COPR_API_TOKEN
copr_url = https://copr.fedorainfracloud.org
EOF
fi

# Create build directory structure
BUILD_DIR="/tmp/rpm-build"
mkdir -p "$BUILD_DIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

# Set up RPM build environment
echo "%_topdir $BUILD_DIR" >~/.rpmmacros
echo "%_tmppath %{_topdir}/tmp" >>~/.rpmmacros

cd "$BUILD_DIR"

echo "=== Creating Source Tarball ==="
# Create original source tarball
git -C "$REPO_ROOT" archive --format=tar --prefix="${PACKAGE_NAME}-${VERSION}/" HEAD >"SOURCES/${PACKAGE_NAME}-${VERSION}.tar"

# Compress the tarball
gzip "SOURCES/${PACKAGE_NAME}-${VERSION}.tar"

echo "=== Vendoring Rust Dependencies ==="
# Extract source for vendoring
cd SOURCES
tar -xzf "${PACKAGE_NAME}-${VERSION}.tar.gz"
cd "${PACKAGE_NAME}-${VERSION}"

# Vendor dependencies
mkdir -p .cargo
cat >.cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

cargo vendor vendor/
tar -czf "../${PACKAGE_NAME}-vendor-${VERSION}.tar.gz" vendor/ .cargo/

# Clean up extracted source
cd ../..
rm -rf "SOURCES/${PACKAGE_NAME}-${VERSION}"

echo "=== Creating RPM Spec File ==="
# Create spec file from template
cat >"SPECS/${PACKAGE_NAME}.spec" <<'EOF'
%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           __PACKAGE_NAME__
Version:        __VERSION__
Release:        1%{?dist}
Summary:        The front-end to your dev env

License:        MIT
URL:            https://mise.jdx.dev
Source0:        https://github.com/jdx/mise/archive/v%{version}/mise-%{version}.tar.gz
Source1:        mise-vendor-%{version}.tar.gz

BuildRequires:  rust >= 1.85
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  git
BuildRequires:  openssl-devel

%description
mise is a development environment setup tool that manages runtime versions,
environment variables, and tasks. It's a replacement for tools like nvm, rbenv,
pyenv, etc. and works with any language.

%prep
%autosetup -n %{name}-%{version}
%setup -q -T -D -a 1

%build
# Set up vendored dependencies
mkdir -p .cargo
cp .cargo/config.toml .cargo/config.toml.bak 2>/dev/null || true
cat > .cargo/config.toml << 'CARGO_EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CARGO_EOF

# Build with specified profile
cargo build --profile __BUILD_PROFILE__ --frozen --bin mise

%install
mkdir -p %{buildroot}%{_bindir}
cp target/__BUILD_PROFILE__/mise %{buildroot}%{_bindir}/

# Install man page if available
mkdir -p %{buildroot}%{_mandir}/man1
cp man/man1/mise.1 %{buildroot}%{_mandir}/man1/

# Install shell completions
mkdir -p %{buildroot}%{_datadir}/bash-completion/completions
cp completions/mise.bash %{buildroot}%{_datadir}/bash-completion/completions/mise

mkdir -p %{buildroot}%{_datadir}/zsh/site-functions
cp completions/_mise %{buildroot}%{_datadir}/zsh/site-functions/

mkdir -p %{buildroot}%{_datadir}/fish/vendor_completions.d
cp completions/mise.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/

# Disable self-update for package manager installations
mkdir -p %{buildroot}%{_libdir}/mise
cat > %{buildroot}%{_libdir}/mise/mise-self-update-instructions.toml <<'TOML'
message = "To update mise from COPR, run:\n\n  sudo dnf upgrade mise\n"
TOML

%files
%license LICENSE
%doc README.md
%{_bindir}/mise
%{_mandir}/man1/mise.1*
%{_datadir}/bash-completion/completions/mise
%{_datadir}/zsh/site-functions/_mise
%{_datadir}/fish/vendor_completions.d/mise.fish
%{_libdir}/mise/mise-self-update-instructions.toml

%changelog
* __CHANGELOG_DATE__ __MAINTAINER_NAME__ <__MAINTAINER_EMAIL__> - %{version}-1
- New upstream release %{version}
EOF

# Replace placeholders in spec file
CHANGELOG_DATE=$(date +'%a %b %d %Y')
sed -i "s/__PACKAGE_NAME__/${PACKAGE_NAME}/g" "SPECS/${PACKAGE_NAME}.spec"
sed -i "s/__VERSION__/${VERSION}/g" "SPECS/${PACKAGE_NAME}.spec"
sed -i "s/__BUILD_PROFILE__/${BUILD_PROFILE}/g" "SPECS/${PACKAGE_NAME}.spec"
sed -i "s/__CHANGELOG_DATE__/${CHANGELOG_DATE}/g" "SPECS/${PACKAGE_NAME}.spec"
sed -i "s/__MAINTAINER_NAME__/${MAINTAINER_NAME}/g" "SPECS/${PACKAGE_NAME}.spec"
sed -i "s/__MAINTAINER_EMAIL__/${MAINTAINER_EMAIL}/g" "SPECS/${PACKAGE_NAME}.spec"

echo "=== Building Source RPM ==="
# Build source RPM
rpmbuild -bs "SPECS/${PACKAGE_NAME}.spec"

# Copy SRPM to accessible location
SRPM_FILE=$(find SRPMS -name "*.src.rpm" -type f | head -1)
if [ -n "$SRPM_FILE" ]; then
	cp "$SRPM_FILE" "$REPO_ROOT/"
	echo "SRPM created: $REPO_ROOT/$(basename "$SRPM_FILE")"
else
	echo "Error: No SRPM file found"
	exit 1
fi

# Submit to COPR if not in dry-run mode
if [ "$DRY_RUN" != "true" ]; then
	echo "=== Submitting to COPR ==="
	echo "Submitting $(basename "$SRPM_FILE") to COPR project $COPR_OWNER/$COPR_PROJECT"

	# Submit build to COPR
	# Build the copr-cli command with multiple --chroot flags
	copr_cmd="copr-cli build"
	# Split CHROOTS into an array to ensure proper word splitting
	IFS=' ' read -ra chroot_array <<<"$CHROOTS"
	for chroot in "${chroot_array[@]}"; do
		copr_cmd="$copr_cmd --chroot $chroot"
	done
	copr_cmd="$copr_cmd $COPR_OWNER/$COPR_PROJECT $SRPM_FILE"

	eval "$copr_cmd"

	echo "Build submitted successfully!"
else
	echo "=== Dry Run Complete ==="
	echo "SRPM built successfully but not submitted to COPR (dry-run mode)"
fi

# Create artifacts directory
mkdir -p "$REPO_ROOT/artifacts"
cp "$SRPM_FILE" "$REPO_ROOT/artifacts/" 2>/dev/null || true
cp "SPECS/${PACKAGE_NAME}.spec" "$REPO_ROOT/artifacts/" 2>/dev/null || true

echo "=== Build Complete ==="
echo "Artifacts available in $REPO_ROOT/artifacts/"
