# 10 – Other distributions

Everything was developed on Fedora Silverblue 45 (kernel 7.3, libcamera 0.7.2, PipeWire 1.6).

## Speakers (any distro with systemd)
`sudo ./audio/install.sh` works anywhere: it only needs Python 3, systemd and the `snd_hda_intel`
hwdep device (`CONFIG_SND_HDA_HWDEP=y`, enabled in every major distro). Without tuned the tuned step
is skipped.

## Cameras on mutable distros (Fedora Workstation, Ubuntu, Arch …)

Requirements: kernel ≥ ~6.8 with `CONFIG_INTEL_SKL_INT3472`, `CONFIG_VIDEO_IPU3_CIO2`,
`CONFIG_VIDEO_IPU3_IMGU` (staging), `CONFIG_V4L2_CCI_I2C`; kernel headers; libcamera ≥ 0.3 built with
the `ipu3` pipeline.

### With DKMS (recommended — rebuilds on every kernel update)
```
sudo cp -r camera/kernel /usr/src/tabpro-s-camera-1.0
sudo dkms install tabpro-s-camera/1.0
sudo install -m 0755 camera/system/w700-camera-load /usr/local/sbin/
sudo install -m 0644 camera/system/w700-camera.service /etc/systemd/system/
sudo systemctl enable w700-camera.service
```
With Secure Boot, enroll DKMS's key once: Fedora `sudo mokutil --import /var/lib/dkms/mok.pub`,
Ubuntu does this automatically when installing DKMS packages.

### Manually
```
make -C camera/kernel                                  # needs kernel headers
make -C camera/kernel sign MOK_KEY=… MOK_CERT=…        # if Secure Boot
sudo ./camera/install.sh                               # → /lib/modules/$(uname -r)/updates/w700-camera
```

### Notes
* The `int3472` and `ipu-bridge` sources here are from Linux **v7.3**. For older kernels copy the
  same files from *your* kernel's source and apply `camera/kernel/*/0001-*.patch` instead.
* If the stock PMIC module is in the initramfs (it usually is), the service swaps it at boot.
  Alternatively regenerate the initramfs after installing (`dracut -f` / `update-initramfs -u`) so the
  `updates/` copy is used directly.

### libcamera
Apply `camera/libcamera/0001-libipa-libcamera-add-Sony-IMX241.patch` to your libcamera source
(paths are relative to the libcamera tree), rebuild, install. On Fedora:
`./camera/libcamera/build-rpm.sh` then `sudo dnf install camera/libcamera/rpms/libcamera-[0-9]*.rpm
camera/libcamera/rpms/libcamera-ipa-*.rpm`.
