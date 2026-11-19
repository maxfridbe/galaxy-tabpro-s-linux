# Linux on the Samsung Galaxy TabPro S (SM-W700)

> **Kernel bug:** [Bug 188411 – No speaker output on Samsung Tabpro S](https://bugzilla.kernel.org/show_bug.cgi?id=188411)
> (open since 2016). Root cause and a working fix are documented here:
> [docs/02-audio-speakers.md](docs/02-audio-speakers.md).

Everything needed to run Linux — tested on **Fedora Silverblue 45** — on the 2016 Samsung Galaxy
TabPro S (model **SM-W700**, Intel Core m3-6Y30 "Skylake", 4 GB RAM, 12" AMOLED), including fixes
for the two things that have never worked on Linux on this tablet:

* **internal speakers** — silent on Linux since 2016 ([kernel bug 188411](https://bugzilla.kernel.org/show_bug.cgi?id=188411))
* **both cameras** — no driver for the sensor, no power-up data for the camera PMIC
* **screen brightness** — the AMOLED ignores every standard backlight control; it needs an unlock
  key and Samsung's gamma commands to the Parade timing controller

All of these were reverse-engineered from the Windows drivers (without redistributing them) and verified on
real hardware.

## Status

| Hardware | Status | Notes |
|---|---|---|
| Display, GPU (Intel HD 515) | ✅ works out of the box | |
| **Screen brightness** (AMOLED, Parade TCON) | ✅ **with [`brightness/`](brightness/)** | real panel brightness via Samsung's TCON commands; per-unit profile — [docs/12](docs/12-brightness.md) |
| Wi-Fi (Qualcomm Atheros QCA6174, ath10k) | ✅ out of the box | |
| Bluetooth (QCA61x4) | ✅ works (discovers devices) | |
| Touchscreen (Synaptics I2C-HID `SYNP1234`) | ✅ out of the box | |
| Keyboard cover | ❓ untested on Linux | a USB keyboard works (needed once for Secure Boot key enrollment) |
| Suspend (s2idle) / resume | ✅ works | power button wakes it |
| Headphone jack | ✅ out of the box | |
| **Internal speakers** (2× Maxim MAX98925) | ✅ **with [`audio/`](audio/)** | stereo, gain configurable |
| Internal microphone (digital, on ALC298) | ✅ works | set input level ≤ 60 % (100 % clips) — see [docs/03](docs/03-microphone.md) |
| **Rear camera** (Sony IMX241 5 MP + DW9806B focus) | ✅ **with [`camera/`](camera/)** | libcamera / PipeWire / GNOME Camera; no AF in apps yet |
| **Front camera** (Sony IMX241 5 MP) | ✅ **with [`camera/`](camera/)** | |
| Accelerometer (ST LIS2HH12, `SMO8A80`) | ✅ **with packages** | auto-rotation (mount matrix from ACPI `ROTM`) |
| Ambient light (ROHM BH1730FVC, `TOBH1733`) | ✅ **with packages** | new driver; GNOME automatic brightness |
| Proximity (Semtech SX9310, `STH0001`) | ✅ driver works | not used by GNOME |
| GNSS (Broadcom BCM4752) | ❌ unsupported | closed Broadcom protocol |
| Pen | ❓ untested | |

## Quick start (Fedora Silverblue / Fedora — RPM packages)

```
sudo curl -Lo /etc/yum.repos.d/tabpro-s.repo https://maxfridbe.github.io/galaxy-tabpro-s-linux/tabpro-s.repo
rpm-ostree install tabpro-s-audio tabpro-s-camera tabpro-s-camera-kmods tabpro-s-brightness
rpm-ostree override replace --experimental --from repo=tabpro-s libcamera libcamera-ipa
systemctl reboot
# only if Secure Boot is enabled (one time, needs a keyboard at the next boot):
sudo tabpro-s-mok-setup && systemctl reboot
```
(Fedora Workstation: `sudo dnf install tabpro-s-audio tabpro-s-camera tabpro-s-camera-kmods libcamera`.)
Packages are rebuilt daily for the newest Fedora kernels and signed (key `380F78FE…BD1C40C8`);
`rpm-ostree upgrade` keeps everything current. Details: [docs/11-packages.md](docs/11-packages.md).
Brightness additionally needs a one-time per-panel profile: [docs/12](docs/12-brightness.md).

## Manual install (from source)

1. **Back up Windows hardware info first** (optional, recommended): [`windows-dump/`](windows-dump/) — a
   small Rust tool that saves the device tree, ACPI tables, registry and drivers to a USB stick.
2. **Install Fedora Silverblue**: [docs/01-install-silverblue.md](docs/01-install-silverblue.md)
   (a 2 GB stick is enough using [`installer/`](installer/)).
3. **Speakers** — no kernel module needed, ~1 minute:
   ```
   git clone https://github.com/maxfridbe/galaxy-tabpro-s-linux && cd galaxy-tabpro-s-linux
   sudo ./audio/install.sh
   ```
4. **Cameras** — needs signed kernel modules and a patched libcamera, ~30 minutes, one reboot with
   a **physical keyboard** attached (Secure Boot key enrollment):
   ```
   ./camera/mok-setup.sh                 # once; reboot with keyboard, enroll the key
   ./camera/build-in-container.sh        # builds + signs the 4 kernel modules
   sudo ./camera/install.sh
   ./camera/libcamera/build-rpm.sh       # patched libcamera (adds the IMX241)
   sudo rpm-ostree override replace camera/libcamera/rpms/libcamera-[0-9]*.rpm \
        camera/libcamera/rpms/libcamera-ipa-*.rpm
   systemctl reboot
   ```
   Full details: [docs/04-camera.md](docs/04-camera.md). **After every kernel update** the modules
   must be rebuilt: [docs/05-kernel-updates.md](docs/05-kernel-updates.md).

## Documentation

| | |
|---|---|
| [01 – Installing Fedora Silverblue](docs/01-install-silverblue.md) | boot from USB, Secure Boot, partitioning, Wi-Fi during install |
| [02 – Speakers](docs/02-audio-speakers.md) | why they are silent and how the fix works |
| [03 – Microphone](docs/03-microphone.md) | levels, clipping, measured response |
| [04 – Cameras](docs/04-camera.md) | architecture, building, installing, testing |
| [05 – Kernel updates](docs/05-kernel-updates.md) | rebuilding the camera modules |
| [06 – Reverse engineering notes](docs/06-reverse-engineering.md) | how every value in this repo was obtained |
| [07 – Hardware reference](docs/07-hardware-reference.md) | ACPI ids, I2C addresses, codec pins, PMIC wiring |
| [08 – Power, suspend, misc](docs/08-power-and-misc.md) | suspend, power button, tuned, sleep settings |
| [09 – Upstreaming](docs/09-upstreaming.md) | what it takes to get this into the kernel / libcamera |
| [10 – Other distributions](docs/10-other-distros.md) | Fedora Workstation, Ubuntu, Arch: DKMS etc. |
| [11 – RPM packages](docs/11-packages.md) | the signed dnf repo, CI, Secure Boot handling |
| [12 – Screen brightness](docs/12-brightness.md) | why the slider did nothing, the Parade TCON unlock, profile generation |

## Repository layout

```
brightness/     AMOLED brightness daemon + systemd unit (needs a per-panel profile, docs/12)
audio/          speaker fix: w700-speakers (Python, no deps), systemd units, install.sh
camera/
  kernel/       imx241 (new), dw9806b (new), int3472 tps68470 + board data, ipu-bridge + INT347F
                Makefile, dkms.conf, *.patch against upstream v7.3
  libcamera/    IMX241 sensor helper/properties patch + Fedora RPM rebuild script
  system/       boot service + module loader + modprobe config
  tools/        raw/NV12 frame → PNG converters used for verification
installer/      Fedora netinstall-on-FAT USB stick: grub.cfg + Silverblue kickstart, linux-collect.sh
windows-dump/   Rust tool: back up the Windows device tree / ACPI / registry / drivers
re/             reverse-engineering tools (disassembly index, Unicorn emulators, HDA verb tool) and
                the extracted data (register tables, PMIC sequences, ACPI evaluation results)
docs/           all the documentation
```

## Licence

Kernel code (`camera/kernel/`, `re/w700acpi.c`) is **GPL-2.0** like the kernel it derives from; the
libcamera patch is **LGPL-2.1-or-later**; everything else is **GPL-2.0-or-later** — see
[LICENSE](LICENSE). No Windows driver binaries or firmware are included; only register values and
sequences (hardware facts) extracted from them.

**No warranty.** The speaker fix drives the amplifiers without the speaker-protection DSP Windows
uses; keep the gain sensible. Use at your own risk.
