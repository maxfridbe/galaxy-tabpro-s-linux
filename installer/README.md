# installer/ – 2 GB USB stick that installs Fedora Silverblue

* `grub.cfg` – replaces `EFI/BOOT/grub.cfg` of the Fedora *Everything netinstall* ISO copied onto a
  FAT stick. Finds the stick by filesystem UUID — **replace `0008-A438` with yours** (`lsblk -o NAME,UUID`).
* `ks-silverblue.cfg` – partial kickstart (`w700/ks-silverblue.cfg` on the stick):
  `ostreecontainer --url=quay.io/fedora/fedora-silverblue:45`. Everything else stays interactive.
* `linux-collect.sh` – run on the installed system to collect hardware info for debugging/bug reports.

Step-by-step: [docs/01-install-silverblue.md](../docs/01-install-silverblue.md).
