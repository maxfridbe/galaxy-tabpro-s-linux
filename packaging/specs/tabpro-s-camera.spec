# Build from a checkout:  rpmbuild -bb --define "repo $PWD" packaging/specs/tabpro-s-camera.spec
Name:           tabpro-s-camera
Version:        1.0
Release:        %{?rel}%{!?rel:1}%{?dist}
Summary:        Camera support (loader/signing) for the Samsung Galaxy TabPro S (SM-W700)
License:        GPL-2.0-or-later
URL:            https://github.com/maxfridbe/galaxy-tabpro-s-linux
BuildArch:      noarch
Requires:       openssl
Requires:       python3
Requires:       kmod
Requires:       coreutils
Requires:       mokutil
Recommends:     tabpro-s-camera-kmods

%description
Boot service that loads the Galaxy TabPro S camera drivers from tabpro-s-camera-kmods: the patched
TPS68470 control-logic driver (board data), ipu-bridge with the IMX241 entry, the Sony IMX241 sensor
driver and the DW9806B lens driver. If the running kernel enforces module signatures (Secure Boot),
the modules are signed locally with the machine owner's key from /etc/pki/tabpro-s (create it with
tabpro-s-mok-setup); without Secure Boot they are loaded unsigned. A libcamera with IMX241 support
is needed for applications (available from the same repository).

%prep
%build

%install
install -D -m 0755 %{repo}/packaging/scripts/tabpro-s-camera-load %{buildroot}%{_libexecdir}/tabpro-s/camera-load
install -D -m 0755 %{repo}/packaging/scripts/tabpro-s-sign-module %{buildroot}%{_libexecdir}/tabpro-s/sign-module
install -D -m 0755 %{repo}/packaging/scripts/tabpro-s-mok-setup %{buildroot}%{_bindir}/tabpro-s-mok-setup
install -D -m 0644 %{repo}/packaging/systemd/tabpro-s-camera.service %{buildroot}%{_unitdir}/tabpro-s-camera.service
install -d %{buildroot}%{_unitdir}/multi-user.target.wants
ln -s ../tabpro-s-camera.service %{buildroot}%{_unitdir}/multi-user.target.wants/tabpro-s-camera.service
install -d %{buildroot}%{_tmpfilesdir}
echo "d /var/lib/tabpro-s-camera 0755 root root -" > %{buildroot}%{_tmpfilesdir}/tabpro-s-camera.conf

%files
%{_libexecdir}/tabpro-s/camera-load
%{_libexecdir}/tabpro-s/sign-module
%{_bindir}/tabpro-s-mok-setup
%{_unitdir}/tabpro-s-camera.service
%{_unitdir}/multi-user.target.wants/tabpro-s-camera.service
%{_tmpfilesdir}/tabpro-s-camera.conf
