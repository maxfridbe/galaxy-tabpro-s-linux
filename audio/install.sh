#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Install the Galaxy TabPro S speaker fix. Works on Fedora Silverblue/Kinoite (ostree) and
# ordinary distros. Run as root:  sudo ./audio/install.sh      (uninstall: sudo ./audio/install.sh --uninstall)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "run as root (sudo)"; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
  systemctl disable --now w700-speakers.service w700-speakers-resume.service 2>/dev/null || true
  rm -f /usr/local/sbin/w700-speakers /etc/systemd/system/w700-speakers{,-resume}.service \
        /etc/modprobe.d/w700-audio.conf
  rm -rf /etc/tuned/profiles/balanced /etc/tuned/profiles/powersave   # only if created by us
  systemctl daemon-reload; echo "removed (keeping /etc/w700-speakers.conf)"; exit 0
fi

dmi="$(cat /sys/class/dmi/id/product_name 2>/dev/null || true)"
[ "$dmi" = "Galaxy TabPro S" ] || echo "WARNING: this is '$dmi', not a Galaxy TabPro S (the script itself refuses to touch other hardware)"

install -D -m 0755 "$here/w700-speakers" /usr/local/sbin/w700-speakers
install -m 0644 "$here/w700-speakers.service" "$here/w700-speakers-resume.service" /etc/systemd/system/
[ -e /etc/w700-speakers.conf ] || install -m 0644 "$here/w700-speakers.conf" /etc/w700-speakers.conf

# The codec must never power down: its internal I2S block (bridge device 0x1b) loses the
# configuration that feeds the amps. Disable HDA runtime power saving.
echo "options snd_hda_intel power_save=0 power_save_controller=N" > /etc/modprobe.d/w700-audio.conf
echo 0 > /sys/module/snd_hda_intel/parameters/power_save 2>/dev/null || true

# tuned (Fedora's default power manager) re-enables audio power save in balanced/powersave
if [ -d /usr/lib/tuned/profiles ] || [ -d /usr/lib/tuned ]; then
  for p in balanced powersave; do
    src=/usr/lib/tuned/profiles/$p/tuned.conf; [ -f "$src" ] || src=/usr/lib/tuned/$p/tuned.conf
    [ -f "$src" ] || continue
    dst=/etc/tuned/profiles/$p/tuned.conf; [ -d /usr/lib/tuned/profiles ] || dst=/etc/tuned/$p/tuned.conf
    install -D -m 0644 "$src" "$dst"
    sed -i '/^\[audio\]/,/^\[/ s/^timeout=.*/timeout=0/' "$dst"
  done
  systemctl try-restart tuned 2>/dev/null || true
fi

command -v restorecon >/dev/null && restorecon -R /usr/local/sbin/w700-speakers /etc/systemd/system/w700-speakers*.service 2>/dev/null || true
systemctl daemon-reload
systemctl enable w700-speakers.service w700-speakers-resume.service
systemctl restart w700-speakers.service
journalctl -u w700-speakers.service -n 3 --no-pager -o cat
echo "done. Speakers should work now and after every boot/resume. Gain: /etc/w700-speakers.conf"
