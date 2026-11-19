#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Create a personal Machine Owner Key for signing the camera modules (Secure Boot), and queue it
# for enrollment. You need a KEYBOARD at the next boot (blue "MOK management" screen):
#   Enroll MOK -> Continue -> Yes -> type the password you choose now -> Reboot
# Keep the private key safe and OFF the tablet if you like; it only needs to exist where you build.
set -euo pipefail
dir="${1:-$HOME/.local/share/tabpro-s-mok}"
mkdir -p "$dir"; chmod 700 "$dir"
if [ ! -f "$dir/mok.key" ]; then
  openssl req -new -x509 -newkey rsa:2048 -nodes -days 36500 \
    -subj "/CN=Galaxy TabPro S local kernel module signing/" -addext "extendedKeyUsage=codeSigning" \
    -keyout "$dir/mok.key" -outform DER -out "$dir/mok.der"
  chmod 600 "$dir/mok.key"
fi
echo "key: $dir/mok.key   cert: $dir/mok.der"
if mokutil --list-enrolled 2>/dev/null | grep -q "Galaxy TabPro S local kernel module signing"; then
  echo "already enrolled"; exit 0
fi
echo "Choose a one-time password for the MOK screen:"
sudo mokutil --import "$dir/mok.der"
echo "Now reboot WITH A KEYBOARD attached and enroll the key (see above)."
