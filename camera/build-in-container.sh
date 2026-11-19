#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build (and optionally sign) the camera modules inside a Fedora container whose kernel-devel matches
# the target kernel. Ideal for Silverblue, where the host cannot install kernel-devel.
#   ./camera/build-in-container.sh [KVER] [MOK_DIR]
#   KVER defaults to `uname -r`; MOK_DIR defaults to ~/.local/share/tabpro-s-mok (from mok-setup.sh)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
KVER="${1:-$(uname -r)}"; MOK="${2:-$HOME/.local/share/tabpro-s-mok}"
case "$KVER" in *.x86_64) ;; *) KVER="$KVER.x86_64" ;; esac   # kernel-devel dirs include the arch
FEDORA="$(echo "$KVER" | sed -n 's/.*\.fc\([0-9]\+\)\..*/\1/p')"; FEDORA="${FEDORA:-latest}"
img="registry.fedoraproject.org/fedora:$FEDORA"
args=(--rm -e KVER="$KVER" -v "$here/kernel:/src:Z")
[ -f "$MOK/mok.key" ] && args+=(-v "$MOK:/mok:ro,Z")
podman run "${args[@]}" "$img" bash -euc "
  dnf install -y -q gcc make elfutils-libelf-devel openssl >/dev/null
  if ! dnf install -y -q kernel-devel-$KVER >/dev/null 2>&1; then
    # repos only carry the newest kernel; older builds live on Fedora's Koji
    K=\${KVER%.x86_64}; V=\${K%%-*}; R=\${K#*-}
    echo \"kernel-devel-$KVER not in repos, fetching from Koji\"
    dnf install -y -q https://kojipkgs.fedoraproject.org/packages/kernel/\$V/\$R/x86_64/kernel-devel-$KVER.rpm >/dev/null
  fi
  make -C /src KVER=$KVER clean >/dev/null 2>&1 || true
  make -C /src KVER=$KVER
  if [ -f /mok/mok.key ]; then make -C /src KVER=$KVER sign MOK_KEY=/mok/mok.key MOK_CERT=/mok/mok.der; fi
"
ls -l "$here"/kernel/*/*.ko
