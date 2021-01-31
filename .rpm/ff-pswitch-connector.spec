%define __spec_install_post %{nil}
%define __os_install_post %{_dbpath}/brp-compress
%define debug_package %{nil}

Name: ff-pswitch-connector
Summary: The native connector for the &#x27;Profile Switcher for Firefox&#x27; extension
Version: @@VERSION@@
Release: @@RELEASE@@%{?dist}
License: GPLv3
Group: Applications/Internet
Source0: %{name}-%{version}.tar.gz

BuildRoot: %{_tmppath}/%{name}-%{version}-%{release}-root

%description
%{summary}

%prep
%setup -q

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a * %{buildroot}

%clean
rm -rf %{buildroot}

%files
%defattr(-,root,root,-)
/usr/lib/mozilla/native-messaging-hosts/ax.nd.profile_switcher_ff.json
%{_bindir}/*
