# 04 – Cameras

Both cameras work through libcamera → PipeWire → GNOME Camera (Snapshot), Firefox, video calls.

| | Rear (world facing) | Front (user facing) |
|---|---|---|
| ACPI | `\_SB.PCI0.LNK0`, `INT347F:00` | `\_SB.PCI0.LNK1`, `INT347F:01` |
| Module | `NSMM4D5` | `CM500RF05SW700` |
| Sensor | Sony **IMX241** (model id `0x0241`) 5 MP | Sony IMX241 5 MP |
| I2C | bus I2C2, addr **0x10** | bus I2C3, addr **0x37** |
| Extras | **DW9806B** VCM @0x0c, EEPROM @0x58 | EEPROM @0x51, fixed focus |
| CSI-2 | port 0, 2 lanes | port 1, 2 lanes |
| Clock | 19.2 MHz from TPS68470 HCLK_A | 19.2 MHz from TPS68470 HCLK_B |

Modes: **2592×1944** (link 499.2 MHz, ~36 fps) and **1296×972** binned (link 249.6 MHz).

## Why it did not work
Four separate things were missing in Linux:

1. **TPS68470 board data.** The sensors are powered by a TI TPS68470 PMIC (`INT3472:04`, I2C2 @0x4d).
   The kernel only knows which regulator/GPIO feeds which sensor from a per-model table in
   `drivers/platform/x86/intel/int3472/tps68470_board_data.c`. There was none for this tablet:
   `int3472-tps68470 i2c-INT3472:04: error -ENODEV: No board-data found for this model`.
   Because the sensors' ACPI `_DEP` points at the PMIC, they were never even enumerated.
2. **ipu-bridge** did not know `INT347F` (it needs the CSI-2 link frequencies).
3. **No IMX241 sensor driver** exists in Linux.
4. **No DW9806B VCM driver** — and without it the rear sensor's async notifier never completes,
   so CIO2 creates no links for *either* camera.
5. **libcamera** has no IMX241 sensor helper (`Failed to create camera sensor helper for imx241`).

## What this repo provides
| path | what |
|---|---|
| `camera/kernel/int3472/` | upstream `intel_skl_int3472_tps68470` + TabPro S board data ([patch](../camera/kernel/int3472/)) |
| `camera/kernel/ipu-bridge/` | upstream `ipu-bridge` + `IPU_SENSOR_CONFIG("INT347F", 2, 499200000, 249600000)` |
| `camera/kernel/imx241/` | new V4L2 sensor driver (CCI regmap, runtime PM, exposure/gain/vblank/flip/test pattern) |
| `camera/kernel/dw9806b/` | VCM driver derived from `dw9807-vcm` + Windows init table + regulators |
| `camera/libcamera/` | patch adding the IMX241 sensor helper/properties + Fedora RPM rebuild script |
| `camera/system/` | boot service that swaps the initramfs-loaded stock PMIC module for the patched one |

### PMIC wiring (board data)
| rail | voltage | consumer |
|---|---|---|
| CORE | 1.200 V | rear `dvdd` |
| ANA | 2.8152 V | rear `avdd` |
| VCM | 2.8152 V | rear VCM `vdd` |
| VIO | 1.8006 V | always on |
| VSIO | 1.8006 V | rear `dovdd`, rear VCM `vsio`, **front `vsio`** (front I2C only works with VSIO on) |
| AUX1 | 1.2132 V | front `dvdd` |
| AUX2 | 1.8006 V | front `dovdd` |

| GPIO | function |
|---|---|
| TPS68470 `s_resetn` (9), active low | rear `reset` |
| TPS68470 `s_enable` (7), active low | rear `powerdown` |
| TPS68470 GPIO 4, active high | front `enable` (external AVDD LDO) |
| TPS68470 GPIO 5, active low | front `reset` |
| TPS68470 GPIO 6, active low | front `powerdown` |

These were recovered by emulating Samsung's `SkcController.sys` (`tps68470::SS*` classes) with the
rail millivolts from `imx241.sys` (1200/2800/1800/2800/1800); the resulting register writes are in
[`re/data/tps68470-poweron.txt`](../re/data/tps68470-poweron.txt). The PLL settings it programs
(`XTALDIV=170 PLLDIV=32 …`) are identical to Linux's `clk-tps68470` 19.2 MHz entry and the voltages match
the Surface Go board data — a good cross-check.

## Building and installing

### 1. Secure Boot signing key (once)
Fedora with Secure Boot only loads signed modules (`lockdown=integrity`). Create and enroll a
Machine Owner Key:
```
./camera/mok-setup.sh            # creates ~/.local/share/tabpro-s-mok/{mok.key,mok.der}, queues enrollment
systemctl reboot                 # WITH A KEYBOARD: Enroll MOK → Continue → Yes → password → Reboot
mokutil --list-enrolled | grep "TabPro S"
```
The blue MokManager screen times out after ~10 s and does not respond to touch. If you miss it the
request is discarded; run `mok-setup.sh` again. (Alternative: disable Secure Boot in the firmware.)

### 2. Build the kernel modules
Silverblue cannot install `kernel-devel` on the host, so the build runs in a Fedora container:
```
./camera/build-in-container.sh   # for the running kernel; signs with the MOK key if present
```
On a mutable distro just `make -C camera/kernel && make -C camera/kernel sign MOK_KEY=… MOK_CERT=…`
or use DKMS ([10-other-distros.md](10-other-distros.md)).

Note: Fedora's repos only keep the **newest** `kernel-devel`; the script falls back to Koji for older ones.

### 3. Install
```
sudo ./camera/install.sh
```
* **Silverblue / ostree:** modules → `/usr/local/lib/w700-camera/` (SELinux label
  `modules_object_t` via `semanage`, otherwise `insmod` is denied by `kmod_t`), `/etc/modprobe.d/w700-camera.conf`
  `install` rules, `/usr/local/sbin/w700-camera-load`, `w700-camera.service`.
* **Mutable distros:** modules → `/lib/modules/$(uname -r)/updates/w700-camera/` + `depmod`.

The service is needed because the stock `intel_skl_int3472_tps68470` is loaded from the
initramfs before `/etc/modprobe.d` is visible; the loader removes it and loads the patched one, then
the sensors appear, `imx241` binds and CIO2 completes.

### 4. Patched libcamera
```
./camera/libcamera/build-rpm.sh                       # ~15 min, output in camera/libcamera/rpms/
sudo rpm-ostree override replace camera/libcamera/rpms/libcamera-[0-9]*.rpm camera/libcamera/rpms/libcamera-ipa-*.rpm
systemctl reboot
```
(`rpm-ostree override reset libcamera libcamera-ipa` undoes it.) The patch adds:
```cpp
class CameraSensorHelperImx241 { blackLevel_ = 4096; gain_ = AnalogueGainLinear{ 0, 256, -1, 256 }; };
{ "imx241", { .unitCellSize = { 1120, 1120 }, .testPatternModes = {...}, .sensorDelays = {2,1,2,2} } }
```

### 5. Use it
Install GNOME Camera: `flatpak install flathub org.gnome.Snapshot`. Or test from a terminal:
```
cam -l                                   # 1: Internal back camera  2: Internal front camera
cam -c 1 -s width=1280,height=960,pixelformat=NV12 --capture=40 --file=/tmp/f-#.bin
python3 camera/tools/nv12png.py /tmp/f-39.bin 1280 960 /tmp/f.png
wpctl status | grep Camera               # Built-in Back Camera / Built-in Front Camera
```
On Silverblue the `cam` tool is easiest from a privileged container:
`sudo podman run --rm -it --privileged -v /dev:/dev -v /run/udev:/run/udev:ro fedora:45` and
`dnf install libcamera-tools ./libcamera*.rpm`.

### Raw capture without libcamera
```
M=/dev/media0
media-ctl -d $M -l '"imx241 2-0010":0 -> "ipu3-csi2 0":0 [1]'
for p in '"imx241 2-0010":0' '"ipu3-csi2 0":0' '"ipu3-csi2 0":1'; do media-ctl -d $M -V "$p [fmt:SRGGB10_1X10/2592x1944]"; done
v4l2-ctl -d $(media-ctl -d $M -e "ipu3-cio2 0") --set-fmt-video=width=2592,height=1944,pixelformat=ip3r \
         --stream-mmap --stream-count=8 --stream-to=/tmp/rear.raw
python3 camera/tools/rawview.py /tmp/rear.raw 2592 1944 RGGB /tmp/rear.png   # unpack IPU3 RAW10, debayer
```
(front camera: `"imx241 3-0037"` → `"ipu3-csi2 1"` → `"ipu3-cio2 1"`.)

## Known limitations
* **Low light:** AE tops out at 1/36 s and 8× analogue gain (`IMX241_AGAIN_MAX 224`). The driver
  could allow longer frame times / higher gain.
* **Colour:** no tuning file (`imx241.yaml`); libcamera's IPU3 IPA uses uncalibrated defaults.
* **Autofocus:** the DW9806B lens is exposed (`V4L2_CID_FOCUS_ABSOLUTE` on its subdev) but libcamera's
  IPU3 pipeline does not drive AF on this platform.
* Only two modes; the Windows driver has two more (2592×1944 @ different PLL, 1296×972 slower).
* EEPROM (OTP calibration data at 0x58 / 0x51) is not read.
