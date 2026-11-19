# Build from a checkout:  rpmbuild -bb --define "repo $PWD" packaging/specs/tabpro-s-gnss.spec
# (needs cargo + network for the libc crate, checkpolicy, policycoreutils, selinux-policy-devel)
%global debug_package %{nil}
%global selinuxtype targeted
Name:           tabpro-s-gnss
Version:        0.1
Release:        %{?rel}%{!?rel:1}%{?dist}
Summary:        GPS/GNSS for the Samsung Galaxy TabPro S (SM-W700): runs Broadcom's engine natively
License:        GPL-2.0-or-later
URL:            https://github.com/maxfridbe/galaxy-tabpro-s-linux
ExclusiveArch:  x86_64
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  checkpolicy
BuildRequires:  policycoreutils
BuildRequires:  selinux-policy-devel
BuildRequires:  systemd-rpm-macros
Requires:       systemd
Recommends:     tabpro-s-camera-kmods
Recommends:     geoclue2
%{?selinux_requires}

%description
The TabPro S has a Broadcom BCM4752 GNSS receiver on ttyS4. It has no positioning firmware; the
position is computed on the host by Broadcom's closed GLL engine (BcmGnss.dll). tabpro-s-gnss is a
small PE loader with the Windows API pieces that engine needs: it loads BcmGnss.dll natively (no Wine),
talks to the chip through the bcm4752 kernel driver (tabpro-s-camera-kmods) and publishes NMEA on
/run/tabpro-s-gnss/nmea.sock, which GeoClue uses (configuration and SELinux rule included).
BcmGnss.dll and BcmGnssConfig.xml are NOT included: copy them from the tablet's own Windows driver
to /var/lib/tabpro-s-gnss/ (see gnss/README.md); without them the service does not start.

%prep
%build
cp -r %{repo}/gnss/tabpro-s-gnss src && rm -rf src/target
cargo build --release --locked --manifest-path src/Cargo.toml
checkmodule -M -m -o tabpro_s_gnss.mod %{repo}/gnss/selinux/tabpro_s_gnss.te
semodule_package -o tabpro_s_gnss.pp -m tabpro_s_gnss.mod

%install
install -D -m 0755 src/target/release/tabpro-s-gnss %{buildroot}%{_bindir}/tabpro-s-gnss
strip %{buildroot}%{_bindir}/tabpro-s-gnss
install -D -m 0644 %{repo}/gnss/systemd/tabpro-s-gnss.service %{buildroot}%{_unitdir}/tabpro-s-gnss.service
install -d %{buildroot}%{_unitdir}/multi-user.target.wants
ln -s ../tabpro-s-gnss.service %{buildroot}%{_unitdir}/multi-user.target.wants/tabpro-s-gnss.service
install -D -m 0644 %{repo}/gnss/geoclue/90-tabpro-s-gnss.conf %{buildroot}%{_sysconfdir}/geoclue/conf.d/90-tabpro-s-gnss.conf
install -D -m 0644 tabpro_s_gnss.pp %{buildroot}%{_datadir}/selinux/packages/%{selinuxtype}/tabpro_s_gnss.pp
install -d %{buildroot}%{_sharedstatedir}/tabpro-s-gnss

%post
%selinux_modules_install -s %{selinuxtype} %{_datadir}/selinux/packages/%{selinuxtype}/tabpro_s_gnss.pp

%postun
if [ $1 -eq 0 ]; then
%selinux_modules_uninstall -s %{selinuxtype} tabpro_s_gnss
fi

%files
%{_bindir}/tabpro-s-gnss
%{_unitdir}/tabpro-s-gnss.service
%{_unitdir}/multi-user.target.wants/tabpro-s-gnss.service
%config(noreplace) %{_sysconfdir}/geoclue/conf.d/90-tabpro-s-gnss.conf
%{_datadir}/selinux/packages/%{selinuxtype}/tabpro_s_gnss.pp
%dir %{_sharedstatedir}/tabpro-s-gnss
