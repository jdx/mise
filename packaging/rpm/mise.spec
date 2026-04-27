%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name: mise
Version: 2026.4.23
Release: 1%{?dist}
Summary: Dev tools, env vars, and tasks in one CLI

License: MIT
URL: https://mise.jdx.dev
Source0: htts://github.com/jdx/mise/archive/v%{version}/mise-%{version}.tar.gz
Source1: mise-vendor-%{version}.tar.gz

BuildRequires:  rust >= 1.91
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  git
BuildRequires:  openssl-devel

%description
mise prepares your development environment before each command runs.

%prep
%autosetup -n %{name}-%{version}
%setup -q -T -D -a 1

%build
mkdir -p .cargo
cat > .cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

cargo build --release --frozen --bin mise

%install
mkdir -p %{buildroot}%{_bindir}
cp target/release/mise %{buildroot}%{_bindir}/

mkdir -p %{buildroot}%{_mandir}/man1
cp man/man1/mise.1 %{buildroot}%{_mandir}/man1/

mkdir -p %{buildroot}%{_datadir}/bash-completion/completions
cp completions/mise.bash %{buildroot}%{_datadir}/bash-completion/completions/mise

mkdir -p %{buildroot}%{_datadir}/zsh/site-functions
cp completions/_mise %{buildroot}%{_datadir}/zsh/site-functions/

mkdir -p %{buildroot}%{_datadir}/fish/vendor_completions.d
cp completions/mise.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/

mkdir -p %{buildroot}%{_libdir}/mise
cat > %{buildroot}%{_libdir}/mise/mise-self-update-instructions.toml << 'TOML'
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
%if 0%{?autochangelog}
# Fedora 40+
%autochangelog
%else
# EPEL9/10
* %(date "+%a %b %d %Y") mise Release Bot <noreply@mise.jdx.dev> - 0-1
- Initial Packit-managed build
%endif
