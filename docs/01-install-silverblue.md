# 01 – Installing Fedora Silverblue on the Galaxy TabPro S

Tested with Fedora Silverblue 45 (kernel 7.3) in November 2026. Other distributions work too;
see [10-other-distros.md](10-other-distros.md).

## What you need

* A USB-C → USB-A adapter (the tablet has a **single USB-C port**). A small hub is nicer because
  you can also attach a keyboard.
* A USB stick (2 GB is enough with the method below; any size works with a normal ISO write).
* Wi-Fi (the netinstall downloads the Silverblue image).
* A keyboard: the Samsung keyboard cover or any USB keyboard. Touch does not work in the firmware
  menus, and you need a keyboard once more later to enroll the camera signing key.

## 0. Save the Windows hardware information first (recommended)

Windows is going to be wiped. Before that, run [`windows-dump`](../windows-dump/) from a USB stick:
it saves the full device tree, the ACPI tables, the registry hardware keys and the third-party
drivers. Everything in this repository was derived from such a dump, and it is useful if
something on your unit differs. Also note the tablet's Windows licence is stored in the firmware
(ACPI `MSDM` table), so a later Windows reinstall activates automatically.

**Set the clock in Windows or the firmware** if it is wrong (ours was 2 years off). A wrong clock
makes the HTTPS download of the Silverblue image fail during installation.

## 1. Make the installer stick

### Option A — normal (stick ≥ 4 GB)
Download the Silverblue ISO from <https://fedoraproject.org/atomic-desktops/silverblue/> and write it
with Fedora Media Writer or `dd`. Done.

### Option B — 2 GB stick, keeps the stick a normal FAT drive
The Silverblue ISO (~3 GB) does not fit on 2 GB, but the *Everything netinstall* ISO (~1.3 GB) does,
and Anaconda can install Silverblue from its OCI image with a one-line kickstart.

1. Download `Fedora-Everything-netinst-x86_64-<ver>.iso` and its `CHECKSUM` from
   `https://dl.fedoraproject.org/pub/fedora/linux/releases/<ver>/Everything/x86_64/iso/` and verify it.
2. Copy the ISO *contents* onto the FAT partition of the stick (e.g. `7z x the.iso -o/run/media/$USER/STICK`).
   Skip the `[BOOT]` directory 7-zip creates.
3. Find the stick's filesystem UUID: `lsblk -o NAME,UUID`.
4. Copy [`installer/ks-silverblue.cfg`](../installer/ks-silverblue.cfg) to `w700/ks-silverblue.cfg`
   on the stick and adjust the image tag (`fedora-silverblue:45`) if needed.
5. Replace `EFI/BOOT/grub.cfg` on the stick with [`installer/grub.cfg`](../installer/grub.cfg) and
   replace every `0008-A438` in it with **your** UUID.

The kickstart is *partial*: it only sets the payload (`ostreecontainer`). Disk, network, language
and time zone stay interactive, so nothing is erased before you confirm.

## 2. Boot the stick

The firmware hot-keys are unreliable on this tablet (F2/F10/Del often do nothing). The reliable way:

1. In Windows: **Shift + Restart** (or Settings → Recovery → Advanced startup → Restart now).
2. **Use a device** → the USB stick.
3. If it is not listed: Troubleshoot → Advanced options → **UEFI Firmware Settings**, disable
   *Fast BIOS Mode*, put USB first.

Secure Boot can stay **on**: Fedora's shim is signed with both the Microsoft 2011 and 2023 UEFI CAs
and this 2016 firmware trusts the 2011 one.

## 3. Install

1. Pick **Install Fedora Silverblue** in the GRUB menu (or "basic graphics" if the screen stays black).
2. **Network & Host Name** → enable Wi-Fi first (the image is downloaded).
3. **Installation Destination** → the 128 GB LITE-ON SSD → *Automatic* → *Reclaim space* →
   *Delete all* → *Reclaim space*. (Or keep Windows and shrink it — untested.)
4. Begin installation. The user account is created on first boot.

## 4. First boot checklist

* `rpm-ostree status` — you are on Silverblue.
* Touch, Wi-Fi, headphones work already.
* Speakers: [02-audio-speakers.md](02-audio-speakers.md).
* Microphone level: [03-microphone.md](03-microphone.md).
* Cameras: [04-camera.md](04-camera.md).
* Power settings: [08-power-and-misc.md](08-power-and-misc.md).

## Collecting Linux-side information

[`installer/linux-collect.sh`](../installer/linux-collect.sh) gathers `dmesg`, `lspci`, ACPI tables,
HDA codec pin configs, HID report descriptors, I2C/ACPI device lists, PipeWire state etc. into a
directory — handy for bug reports. Run it with `sudo bash linux-collect.sh`.
