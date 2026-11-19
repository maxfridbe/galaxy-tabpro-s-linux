#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Install already-built camera modules + boot service. Run as root from the repo:
#   sudo ./camera/install.sh            (uninstall: sudo ./camera/install.sh --uninstall)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "run as root (sudo)"; exit 1; }
mods=(kernel/int3472/intel_skl_int3472_tps68470.ko kernel/ipu-bridge/ipu-bridge.ko kernel/imx241/imx241.ko kernel/dw9806b/dw9806b.ko)
ostree=0; [ -e /run/ostree-booted ] && ostree=1

if [ "${1:-}" = "--uninstall" ]; then
  systemctl disable --now w700-camera.service 2>/dev/null || true
  rm -f /etc/systemd/system/w700-camera.service /usr/local/sbin/w700-camera-load /etc/modprobe.d/w700-camera.conf
  rm -rf /usr/local/lib/w700-camera /lib/modules/*/updates/w700-camera 2>/dev/null || true
  [ $ostree = 0 ] && depmod -a
  systemctl daemon-reload; echo "camera stack removed (reboot to fall back to stock modules)"; exit 0
fi

for m in "${mods[@]}"; do [ -f "$here/$m" ] || { echo "missing $m - build first (see docs/04-camera.md)"; exit 1; }; done
kver="$(modinfo -F vermagic "$here/kernel/imx241/imx241.ko" | cut -d" " -f1)"
[ "$kver" = "$(uname -r)" ] || echo "WARNING: modules built for $kver but running $(uname -r)"
if mokutil --sb-state 2>/dev/null | grep -q enabled && ! tail -c 28 "$here/kernel/imx241/imx241.ko" | grep -q "Module signature appended"; then
  echo "WARNING: Secure Boot is on but modules are unsigned - they will be rejected (see camera/mok-setup.sh)"
fi

if [ $ostree = 1 ]; then
  # /usr is read-only: keep modules in /usr/local and let SELinux treat them as kernel modules
  install -D -m 0644 -t /usr/local/lib/w700-camera "${mods[@]/#/$here/}"
  if command -v semanage >/dev/null; then
    semanage fcontext -l | grep '/usr/local/lib/w700-camera' >/dev/null || \
      semanage fcontext -a -t modules_object_t '/usr/local/lib/w700-camera(/.*)?'
    restorecon -R /usr/local/lib/w700-camera
  else
    chcon -R -t modules_object_t /usr/local/lib/w700-camera 2>/dev/null || true
  fi
  install -m 0644 "$here/system/w700-camera.conf" /etc/modprobe.d/w700-camera.conf
else
  # mutable distro: modules in updates/ override the in-tree ones and autoload via aliases
  dst="/lib/modules/$(uname -r)/updates/w700-camera"
  install -D -m 0644 -t "$dst" "${mods[@]/#/$here/}"
  depmod -a
fi
install -D -m 0755 "$here/system/w700-camera-load" /usr/local/sbin/w700-camera-load
install -m 0644 "$here/system/w700-camera.service" /etc/systemd/system/w700-camera.service
command -v restorecon >/dev/null && restorecon -R /usr/local/sbin/w700-camera-load /etc/systemd/system/w700-camera.service 2>/dev/null || true
systemctl daemon-reload
systemctl enable w700-camera.service
echo "installed. Reboot, then: cam -l   (needs the patched libcamera, see camera/libcamera/)"
