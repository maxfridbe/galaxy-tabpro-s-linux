# 05 – Kernel updates

The **speaker fix** is userspace only and survives kernel updates.

The **camera modules** are built for one exact kernel. After `rpm-ostree upgrade` (or `dnf upgrade`)
boots a new kernel the cameras disappear — everything else keeps working — until you rebuild:

```
cd galaxy-tabpro-s-linux
git pull                                 # pick up any source refresh
./camera/build-in-container.sh           # builds + signs for `uname -r`
sudo ./camera/install.sh
systemctl reboot
```

You can build for the *next* kernel before rebooting into it: `rpm-ostree status` shows the staged
deployment's kernel version; pass it as `./camera/build-in-container.sh <KVER>`. Note
`install.sh` installs into a kernel-agnostic location on Silverblue, so install after rebooting
(it warns if the vermagic does not match the running kernel).

## If the build fails on a new kernel
`camera/kernel/int3472/` and `camera/kernel/ipu-bridge/` are full copies of upstream files (v7.3)
with small patches. If the in-kernel API changed:

1. fetch the new upstream files for your kernel tag
   (`drivers/platform/x86/intel/int3472/tps68470*.{c,h}`, `drivers/media/pci/intel/ipu-bridge.c`),
2. re-apply `camera/kernel/*/0001-*.patch`,
3. rebuild.

`imx241.c` and `dw9806b.c` follow current mainline sensor-driver conventions (`v4l2-cci`,
`devm_v4l2_sensor_clk_get`) and normally only break on V4L2 API changes.

## libcamera
The rpm-ostree override pins libcamera. When Fedora ships a newer libcamera, rebuild with
`./camera/libcamera/build-rpm.sh` (it picks up the installed version) and replace the override.
To go back to stock: `sudo rpm-ostree override reset libcamera libcamera-ipa`.

## Making this go away
Once the pieces are upstream (see [09-upstreaming.md](09-upstreaming.md)) none of this is needed.
