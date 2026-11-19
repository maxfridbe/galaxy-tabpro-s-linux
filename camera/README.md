# camera/ – rear + front camera support

```
./camera/mok-setup.sh                 # once: signing key for Secure Boot (needs a keyboard at next boot)
./camera/build-in-container.sh        # build + sign kernel modules for the running kernel
sudo ./camera/install.sh              # install modules + boot service
./camera/libcamera/build-rpm.sh       # build libcamera with the IMX241 helper
sudo rpm-ostree override replace camera/libcamera/rpms/libcamera-[0-9]*.rpm camera/libcamera/rpms/libcamera-ipa-*.rpm
systemctl reboot
```

| dir | |
|---|---|
| `kernel/imx241/` | **new** Sony IMX241 V4L2 driver; `imx241_modes.h` = register tables from the Windows driver |
| `kernel/dw9806b/` | **new** DW9806B VCM (lens) driver, derived from `dw9807-vcm.c` |
| `kernel/int3472/` | `intel_skl_int3472_tps68470` from Linux v7.3 + TabPro S board data (`0001-*.patch` = the diff) |
| `kernel/ipu-bridge/` | `ipu-bridge` from Linux v7.3 + `INT347F` entry (`0001-*.patch`) |
| `kernel/Makefile`, `kernel/dkms.conf` | build all four / DKMS |
| `libcamera/` | IMX241 sensor-helper patch, Fedora RPM rebuild script |
| `system/` | `w700-camera-load` (boot loader script), `w700-camera.service`, modprobe `install` rules |
| `tools/` | `rawview.py` (IPU3 packed RAW10 → stats + PNG), `nv12png.py` |

Everything is explained in [docs/04-camera.md](../docs/04-camera.md); kernel updates in
[docs/05-kernel-updates.md](../docs/05-kernel-updates.md).
