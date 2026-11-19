#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Run on the W700 *after* Silverblue is installed, with this stick plugged in:
#   sudo bash /run/media/$USER/*/linux-collect.sh
# Saves hardware info next to this script in linux-backup/<date>/ for review.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/linux-backup/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"/{acpi,hid}
cd "$out"
r() { local f=$1; shift; echo "+ $*"; "$@" >"$f" 2>&1 || echo "  (failed: $?)"; }
r uname.txt uname -a
r os-release.txt cat /etc/os-release
r rpm-ostree.txt rpm-ostree status -v
r cmdline.txt cat /proc/cmdline
r dmesg.txt dmesg
r journal-boot.txt journalctl -b --no-pager
r lspci.txt lspci -nnvvv
r lsusb.txt lsusb -v
r lsmod.txt lsmod
r input-devices.txt cat /proc/bus/input/devices
r libinput.txt libinput list-devices
r i2c-devices.txt bash -c 'for d in /sys/bus/i2c/devices/*; do echo "$d $(cat $d/name 2>/dev/null) $(readlink -f $d/driver)"; done'
r serio.txt bash -c 'for d in /sys/bus/serio/devices/*; do echo "$d"; cat $d/description $d/id/* 2>/dev/null; done'
r tty-pnp.txt bash -c 'for d in /sys/bus/pnp/devices/*; do echo "$d $(cat $d/id) $(readlink -f $d/driver)"; done'
r acpi-devices.txt bash -c 'for d in /sys/bus/acpi/devices/*; do echo "$(basename $d) $(cat $d/hid 2>/dev/null) status=$(cat $d/status 2>/dev/null) $(readlink -f $d/physical_node 2>/dev/null)"; done'
r asound-cards.txt cat /proc/asound/cards
r asound-all.txt bash -c 'for f in /proc/asound/card*/codec*; do echo "=== $f"; cat "$f"; done'
r hda-pins.txt bash -c 'for d in /sys/class/sound/hwC*; do echo "=== $d"; for f in vendor_name chip_name init_pin_configs driver_pin_configs user_pin_configs hints; do echo "-- $f"; cat $d/$f; done; done'
r aplay.txt aplay -l
r arecord.txt arecord -l
r pactl.txt pactl list
r wpctl.txt wpctl status
for d in /sys/bus/hid/devices/*; do n=$(basename "$d"); cp "$d/report_descriptor" "hid/$n.rdesc" 2>/dev/null; readlink -f "$d/driver" > "hid/$n.driver" 2>/dev/null; done
if command -v acpidump >/dev/null; then (cd acpi && acpidump -b); else cp -r /sys/firmware/acpi/tables/* acpi/ 2>/dev/null; fi
r dmi.txt bash -c 'for f in /sys/class/dmi/id/*; do [ -f "$f" ] && echo "$(basename $f): $(cat $f 2>/dev/null)"; done'
sync
echo "Saved to $out"
