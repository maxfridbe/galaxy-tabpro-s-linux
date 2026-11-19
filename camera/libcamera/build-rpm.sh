#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Rebuild Fedora's libcamera package with the Sony IMX241 sensor helper/properties patch.
#   ./camera/libcamera/build-rpm.sh [FEDORA_RELEASE] [LIBCAMERA_NVR]
#   defaults: the host's Fedora release and installed libcamera version.
# Output RPMs land in camera/libcamera/rpms/. Then install them:
#   Silverblue: sudo rpm-ostree override replace rpms/libcamera-[0-9]*.rpm rpms/libcamera-ipa-*.rpm && reboot
#   Fedora:     sudo dnf install rpms/libcamera-[0-9]*.rpm rpms/libcamera-ipa-*.rpm
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
REL="${1:-$(rpm -E %fedora)}"
NVR="${2:-$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}' libcamera 2>/dev/null | sed 's/\.w700\.[0-9]*//')}"
mkdir -p "$here/rpms"
podman run --rm -v "$here:/w:Z" "registry.fedoraproject.org/fedora:$REL" bash -euc "
  dnf install -y -q rpm-build 'dnf-command(download)' 'dnf-command(builddep)' >/dev/null
  cd /tmp && dnf download -q --source $NVR
  rpm -i --define '_topdir /tmp/rb' $NVR.src.rpm 2>/dev/null
  cp /w/0001-libipa-libcamera-add-Sony-IMX241.patch /tmp/rb/SOURCES/
  spec=/tmp/rb/SPECS/libcamera.spec
  sed -i 's/^Release:\(\s*\)\([0-9.]*\)%{?dist}/Release:\1\2.w700.1%{?dist}/' \$spec
  sed -i '0,/^Patch[0-9]*:/s//Patch900: 0001-libipa-libcamera-add-Sony-IMX241.patch\n&/' \$spec
  grep -q 'Patch900' \$spec || sed -i '/^Source0*:/a Patch900: 0001-libipa-libcamera-add-Sony-IMX241.patch' \$spec
  grep -E '^(Release|Patch)' \$spec
  dnf builddep -y -q \$spec >/dev/null
  rpmbuild -bb --define '_topdir /tmp/rb' \$spec >/tmp/build.log 2>&1 || { tail -40 /tmp/build.log; exit 1; }
  cp /tmp/rb/RPMS/*/*.rpm /w/rpms/
"
ls -1 "$here/rpms" | grep -v debug
