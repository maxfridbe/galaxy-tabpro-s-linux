# Build from a checkout:  rpmbuild -bb --define "repo $PWD" packaging/specs/tabpro-s-audio.spec
Name:           tabpro-s-audio
Version:        1.0
Release:        %{?rel}%{!?rel:1}%{?dist}
Summary:        Internal speaker support for the Samsung Galaxy TabPro S (SM-W700)
License:        GPL-2.0-or-later
URL:            https://github.com/maxfridbe/galaxy-tabpro-s-linux
BuildArch:      noarch
Requires:       python3
Requires:       systemd

%description
Enables the two Maxim MAX98925 speaker amplifiers that sit behind the Realtek ALC298 codec of the
Samsung Galaxy TabPro S (codec subsystem 144d:c135), which Linux leaves switched off (kernel bug
188411). Runs at boot and after resume; does nothing on other hardware. Disables HDA codec power
saving (also in tuned's balanced/powersave profiles), which would otherwise drop the configuration.

%prep
%build

%install
install -D -m 0755 %{repo}/audio/w700-speakers %{buildroot}%{_libexecdir}/tabpro-s/speakers
install -D -m 0644 %{repo}/packaging/systemd/tabpro-s-speakers.service %{buildroot}%{_unitdir}/tabpro-s-speakers.service
install -D -m 0644 %{repo}/packaging/systemd/tabpro-s-speakers-resume.service %{buildroot}%{_unitdir}/tabpro-s-speakers-resume.service
install -D -m 0644 %{repo}/audio/w700-speakers.conf %{buildroot}%{_sysconfdir}/tabpro-s/speakers.conf
install -d %{buildroot}%{_prefix}/lib/modprobe.d
echo "options snd_hda_intel power_save=0 power_save_controller=N" > %{buildroot}%{_prefix}/lib/modprobe.d/tabpro-s-audio.conf
# enabled statically (works for rpm-ostree layering without presets)
install -d %{buildroot}%{_unitdir}/multi-user.target.wants
ln -s ../tabpro-s-speakers.service %{buildroot}%{_unitdir}/multi-user.target.wants/tabpro-s-speakers.service
for t in suspend hibernate hybrid-sleep suspend-then-hibernate; do
  install -d %{buildroot}%{_unitdir}/$t.target.wants
  ln -s ../tabpro-s-speakers-resume.service %{buildroot}%{_unitdir}/$t.target.wants/tabpro-s-speakers-resume.service
done

%files
%{_libexecdir}/tabpro-s/speakers
%{_unitdir}/tabpro-s-speakers.service
%{_unitdir}/tabpro-s-speakers-resume.service
%{_unitdir}/*.target.wants/tabpro-s-speakers*.service
%dir %{_sysconfdir}/tabpro-s
%config(noreplace) %{_sysconfdir}/tabpro-s/speakers.conf
%{_prefix}/lib/modprobe.d/tabpro-s-audio.conf
