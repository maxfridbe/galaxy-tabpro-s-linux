#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build all Galaxy TabPro S RPMs and a dnf repository. Runs as root inside a Fedora container
# (GitHub Actions, or locally: podman run --rm -v $PWD:/src:Z fedora:45 /src/packaging/ci/build-repo.sh).
# Env: NKERNELS (default 4)  LIBCAMERA_CACHE (dir with previously built libcamera RPMs)
#      RPM_GPG_PRIVATE_KEY (armored key; RPMs are signed if set)  OUT (default ./site)
set -euo pipefail
SRC="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${OUT:-$SRC/site}"
NKERNELS="${NKERNELS:-4}"
CACHE="${LIBCAMERA_CACHE:-$SRC/.libcamera-cache}"
REL="$(rpm -E %fedora)"
ARCHDIR="$OUT/fedora/$REL/x86_64"
mkdir -p "$ARCHDIR" "$CACHE"

dnf install -y -q rpm-build rpm-sign createrepo_c gcc make elfutils-libelf-devel openssl git gnupg2 \
  python3 cargo rust checkpolicy policycoreutils selinux-policy-devel systemd-rpm-macros fedora-repos-archive 'dnf-command(download)' 'dnf-command(builddep)' >/dev/null

git config --global --add safe.directory "$SRC"
COUNT="$(git -C "$SRC" rev-list --count HEAD 2>/dev/null || echo 1)"

# ---- kernels: newest $NKERNELS kernel-devel builds for this Fedora release (incl. updates-archive)
mapfile -t KVERS < <(dnf repoquery -q --qf '%{version}-%{release}.%{arch}\n' kernel-devel 2>/dev/null \
                     | grep "\.fc$REL\.x86_64$" | sort -uV | tail -n "$NKERNELS")
echo "kernels: ${KVERS[*]}"
for k in "${KVERS[@]}"; do dnf install -y -q --setopt=installonly_limit=0 "kernel-devel-$k" >/dev/null || { echo "cannot install kernel-devel-$k"; exit 1; }; done
ls /usr/src/kernels
LATEST="${KVERS[-1]%.fc*}"; LATEST="${LATEST//-/.}"

B="$(mktemp -d)"
rb() { rpmbuild -bb --define "_topdir $B" --define "repo $SRC" "$@"; }
rb --define "rel $COUNT" "$SRC/packaging/specs/tabpro-s-audio.spec" >/dev/null
rb --define "rel $COUNT" "$SRC/packaging/specs/tabpro-s-camera.spec" >/dev/null
rb --define "rel $COUNT" "$SRC/packaging/specs/tabpro-s-brightness.spec" >/dev/null
rb --define "rel $COUNT" "$SRC/packaging/specs/tabpro-s-gnss.spec" >/dev/null
rb --define "rel $COUNT.k$LATEST" --define "kvers ${KVERS[*]}" "$SRC/packaging/specs/tabpro-s-camera-kmods.spec" >/dev/null

# ---- libcamera with the IMX241 patch (rebuilt only when Fedora's libcamera or the patch changes)
NVR="$(dnf repoquery -q --latest-limit 1 --qf '%{name}-%{version}-%{release}\n' libcamera.x86_64 | tail -1)"
PH="$(sha256sum "$SRC/camera/libcamera/0001-libipa-libcamera-add-Sony-IMX241.patch" | cut -c1-12)"
STAMP="$CACHE/$NVR.$PH"
if [ ! -f "$STAMP" ]; then
  rm -rf "${CACHE:?}"/*
  cd /tmp && dnf download -q --source "$NVR"
  rpm -i --define "_topdir /tmp/lc" "$NVR.src.rpm" 2>/dev/null
  cp "$SRC/camera/libcamera/0001-libipa-libcamera-add-Sony-IMX241.patch" /tmp/lc/SOURCES/
  spec=/tmp/lc/SPECS/libcamera.spec
  sed -i 's/^Release:\(\s*\)\([0-9.]*\)%{?dist}/Release:\1\2.w700.1%{?dist}/' "$spec"
  sed -i '0,/^Patch[0-9]*:/s//Patch900: 0001-libipa-libcamera-add-Sony-IMX241.patch\n&/' "$spec"
  grep -q Patch900 "$spec" || sed -i '/^Source0*:/a Patch900: 0001-libipa-libcamera-add-Sony-IMX241.patch' "$spec"
  dnf builddep -y -q "$spec" >/dev/null
  rpmbuild -bb --define "_topdir /tmp/lc" "$spec" >/tmp/libcamera-build.log 2>&1 || { tail -50 /tmp/libcamera-build.log; exit 1; }
  find /tmp/lc/RPMS -name '*.rpm' ! -name '*debug*' -exec cp {} "$CACHE/" \;
  touch "$STAMP"
fi
cp "$CACHE"/*.rpm "$ARCHDIR/"
find "$B/RPMS" -name '*.rpm' -exec cp {} "$ARCHDIR/" \;

# ---- sign + repo metadata
if [ -n "${GITHUB_ACTIONS:-}" ] && [ -z "${RPM_GPG_PRIVATE_KEY:-}" ]; then
  echo "RPM_GPG_PRIVATE_KEY secret is not set - refusing to publish unsigned packages"; exit 1
fi
if [ -n "${RPM_GPG_PRIVATE_KEY:-}" ]; then
  export GNUPGHOME="$(mktemp -d)"
  echo "$RPM_GPG_PRIVATE_KEY" | gpg --batch --import 2>/dev/null
  KEYID="$(gpg --list-secret-keys --with-colons | awk -F: '/^sec/{print $5; exit}')"
  cat > ~/.rpmmacros <<MACROS
%_openpgp_sign_id $KEYID
%_gpg_name $KEYID
%_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase ""
MACROS
  rpmsign --addsign "$ARCHDIR"/*.rpm >/dev/null
  gpg --armor --export "$KEYID" > "$OUT/RPM-GPG-KEY-tabpro-s"
fi
createrepo_c -q "$ARCHDIR"
cp "$SRC/packaging/tabpro-s.repo" "$OUT/"
{ echo "<html><body><h1>Galaxy TabPro S Linux packages</h1><p>See <a href='https://github.com/maxfridbe/galaxy-tabpro-s-linux'>the repository</a>.</p>"
  echo "<p>Kernels with camera modules: ${KVERS[*]}</p><ul>"
  for f in "$ARCHDIR"/*.rpm; do echo "<li>$(basename "$f")</li>"; done; echo "</ul></body></html>"; } > "$OUT/index.html"
ls -1 "$ARCHDIR"
