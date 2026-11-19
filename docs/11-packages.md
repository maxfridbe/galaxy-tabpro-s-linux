# 11 – RPM packages (recommended)

| package | contents |
|---|---|
| `tabpro-s-audio` (noarch) | speaker fix (`/usr/libexec/tabpro-s/speakers`), boot + resume services, `/etc/tabpro-s/speakers.conf` (gain), HDA `power_save=0` |
| `tabpro-s-camera` (noarch) | `tabpro-s-camera.service` + loader, openssl module signer, `tabpro-s-mok-setup`, accelerometer mount-matrix hwdb |
| `tabpro-s-camera-kmods` (x86_64) | **unsigned** modules for the newest 4 Fedora kernels: TPS68470 board data, ipu-bridge, imx241, dw9806b, st_accel_i2c, sx9310, bh1730 |
| `tabpro-s-brightness` (noarch) | AMOLED brightness daemon (Parade TCON); needs `/etc/tabpro-s/amoled-profile.json`, see [12](12-brightness.md) |
| `libcamera`, `libcamera-ipa` (`*.w700.1`) | libcamera with the IMX241 sensor helper |

## How the kernel modules work
* The kmods package stores modules in `/usr/lib/tabpro-s-camera/<kernel>/` and does **not** depend on
  a specific kernel — an `rpm-ostree upgrade` is never blocked. If you boot a kernel that has no build
  yet, the cameras/sensors stay off (logged) until the next daily build; audio is unaffected.
* At boot `tabpro-s-camera.service` checks whether the kernel enforces module signatures
  (`/sys/kernel/security/lockdown` = integrity/confidentiality, or `module.sig_enforce`):
  * **Secure Boot off:** modules are loaded as they are. Nothing to set up.
  * **Secure Boot on:** modules are signed **on your machine** with your own Machine Owner Key from
    `/etc/pki/tabpro-s/` (pure `openssl cms`, same format as the kernel's `sign-file`), cached in
    `/var/lib/tabpro-s-camera/<kernel>/`, labelled `modules_object_t`, and loaded. Create and enroll the
    key once with `sudo tabpro-s-mok-setup` (needs a keyboard at the blue MokManager screen).
* Trade-off: with Secure Boot on, your MOK private key lives on the tablet (root-only). Someone who
  already has root could use it to sign their own modules. No key is ever held by the repository/CI.

## Repository
`https://maxfridbe.github.io/galaxy-tabpro-s-linux/`, built by GitHub Actions
(`.github/workflows/rpms.yml`, `packaging/ci/build-repo.sh`) daily and on every push, RPMs signed with
the key in `packaging/RPM-GPG-KEY-tabpro-s` (fingerprint `380F 78FE B737 E7B1 2478 2122 BFCE F63B BD1C 40C8`).
Release numbers change only when the repo changes or a new kernel appears, so you don't get daily
no-op updates.

## Troubleshooting
```
journalctl -b -u tabpro-s-camera -u tabpro-s-speakers
tabpro-s-mok-setup --status
ls /usr/lib/tabpro-s-camera/          # kernels with module builds
```
