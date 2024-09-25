Summary: The front-end to your dev env
Name: mise
Version: 2024.9.9
Release: 1
URL: https://github.com/jdx/mise/
Group: System
License: MIT
Packager: @jdx
BuildRoot: /root/mise

%description
mise is a polyglot runtime manager

%install
mkdir -p %{buildroot}/usr/bin/
cp /root/mise/target/release/mise %{buildroot}/usr/bin
cp /root/mise/man/man1/mise.1 %{buildroot}/%{_mandir}/man1

%files
/usr/bin/mise
%{_mandir}/man1/mise.1
