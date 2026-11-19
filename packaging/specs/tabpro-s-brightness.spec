# Build from a checkout:  rpmbuild -bb --define "repo $PWD" packaging/specs/tabpro-s-brightness.spec
Name:           tabpro-s-brightness
Version:        1.0
Release:        %{?rel}%{!?rel:1}%{?dist}
Summary:        AMOLED screen brightness for the Samsung Galaxy TabPro S (SM-W700)
License:        GPL-2.0-or-later
URL:            https://github.com/maxfridbe/galaxy-tabpro-s-linux
BuildArch:      noarch
Requires:       python3
Requires:       systemd

%description
The TabPro S AMOLED panel ignores the PWM backlight and VESA DPCD brightness. This daemon unlocks
the panel's Parade timing controller and sends Samsung's per-level gamma/luminance commands,
following /sys/class/backlight/intel_backlight (GNOME slider, brightness keys).
It needs a per-panel profile in /etc/tabpro-s/amoled-profile.json, generated once on the tablet
with re/samo-profile.py from the repository (see docs/12-brightness.md); without it the service
does not start.

%prep
%build

%install
install -D -m 0755 %{repo}/brightness/tabpro-s-brightness %{buildroot}%{_libexecdir}/tabpro-s/brightness
sed 's#/usr/local/sbin/tabpro-s-brightness#%{_libexecdir}/tabpro-s/brightness#' \
    %{repo}/brightness/tabpro-s-brightness.service > tabpro-s-brightness.service
install -D -m 0644 tabpro-s-brightness.service %{buildroot}%{_unitdir}/tabpro-s-brightness.service
install -d %{buildroot}%{_unitdir}/graphical.target.wants
ln -s ../tabpro-s-brightness.service %{buildroot}%{_unitdir}/graphical.target.wants/tabpro-s-brightness.service
install -d %{buildroot}%{_sysconfdir}/tabpro-s

%files
%{_libexecdir}/tabpro-s/brightness
%{_unitdir}/tabpro-s-brightness.service
%{_unitdir}/graphical.target.wants/tabpro-s-brightness.service
%dir %{_sysconfdir}/tabpro-s
