Summary: Polyglot runtime manager
Name: rtx
Version: 2023.9.0
Release: 1
URL: https://github.com/jdx/rtx/
Group: System
License: MIT
Packager: @jdx
BuildRoot: /root/rtx

%description
RTX is a polyglot runtime manager

%install
mkdir -p %{buildroot}/usr/bin/
cp /root/rtx/target/release/rtx %{buildroot}/usr/bin
cp /root/rtx/man/man1/rtx.1 %{buildroot}/%{_mandir}/man1

%files
/usr/bin/rtx
%{_mandir}/man1/rtx.1
