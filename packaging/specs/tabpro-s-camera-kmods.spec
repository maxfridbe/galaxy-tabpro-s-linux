# Build from a checkout (kernel-devel for every listed kernel must be installed):
#   rpmbuild -bb --define "repo $PWD" --define "kvers 7.3.4-300.fc45.x86_64 7.3.5-300.fc45.x86_64" \
#            packaging/specs/tabpro-s-camera-kmods.spec
# One package carries modules for several kernels and does NOT require a specific kernel, so an
# rpm-ostree upgrade is never blocked by a missing build; tabpro-s-camera just skips unknown kernels.
%global debug_package %{nil}
%global __strip /bin/true
Name:           tabpro-s-camera-kmods
Version:        1.0
Release:        %{?rel}%{!?rel:1}%{?dist}
Summary:        Galaxy TabPro S camera kernel modules (unsigned) for recent Fedora kernels
License:        GPL-2.0-only
URL:            https://github.com/maxfridbe/galaxy-tabpro-s-linux
ExclusiveArch:  x86_64
BuildRequires:  gcc
BuildRequires:  make
Requires:       tabpro-s-camera

%description
Out-of-tree kernel modules for the Samsung Galaxy TabPro S cameras, built for these kernels:
%{kvers}
Modules: intel_skl_int3472_tps68470 (with TabPro S board data), ipu-bridge (with INT347F),
imx241, dw9806b. They are stored outside /lib/modules and loaded (signed locally if Secure Boot
requires it) by tabpro-s-camera.service.

%prep
%build
for k in %{kvers}; do
  rm -rf build-$k && cp -r %{repo}/camera/kernel build-$k
  make -C build-$k KVER=$k KDIR=/usr/src/kernels/$k
done

%install
for k in %{kvers}; do
  install -d %{buildroot}%{_prefix}/lib/tabpro-s-camera/$k
  install -m 0644 build-$k/*/*.ko %{buildroot}%{_prefix}/lib/tabpro-s-camera/$k/
done

%files
%{_prefix}/lib/tabpro-s-camera
