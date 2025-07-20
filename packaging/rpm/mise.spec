%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           mise
Version:        2025.7.17
Release:        1%{?dist}
Summary:        The front-end to your dev env

License:        MIT
URL:            https://mise.jdx.dev
Source0:        https://github.com/jdx/mise/archive/v%{version}/mise-%{version}.tar.gz

BuildRequires:  rust >= 1.85
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  git

%description
mise is a development environment setup tool that manages runtime versions,
environment variables, and tasks. It's a replacement for tools like nvm, rbenv,
pyenv, etc. and works with any language.

%prep
%autosetup

%build
cargo build --release --bin mise

%install
mkdir -p %{buildroot}%{_bindir}
cp target/release/mise %{buildroot}%{_bindir}/

# Install man page if available
if [ -f man/man1/mise.1 ]; then
  mkdir -p %{buildroot}%{_mandir}/man1
  cp man/man1/mise.1 %{buildroot}%{_mandir}/man1/
fi

# Install shell completions
mkdir -p %{buildroot}%{_datadir}/bash-completion/completions
if [ -f completions/mise.bash ]; then
  cp completions/mise.bash %{buildroot}%{_datadir}/bash-completion/completions/mise
fi

mkdir -p %{buildroot}%{_datadir}/zsh/site-functions
if [ -f completions/_mise ]; then
  cp completions/_mise %{buildroot}%{_datadir}/zsh/site-functions/
fi

mkdir -p %{buildroot}%{_datadir}/fish/vendor_completions.d
if [ -f completions/mise.fish ]; then
  cp completions/mise.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/
fi

%files
%license LICENSE
%doc README.md
%{_bindir}/mise
%{_mandir}/man1/mise.1*
%{_datadir}/bash-completion/completions/mise
%{_datadir}/zsh/site-functions/_mise
%{_datadir}/fish/vendor_completions.d/mise.fish

%changelog
* Mon Jan 20 2025 mise Release Bot <noreply@mise.jdx.dev> - 2025.7.17-1
- New upstream release 2025.7.17
