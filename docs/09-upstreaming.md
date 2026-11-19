# 09 – Upstreaming

Everything here works, but it is carried out-of-tree. To make the TabPro S work on any distro with no
extra steps, each piece has to go to its upstream project. None of them use GitHub pull requests:
the kernel and libcamera take patches by e-mail (`git format-patch` + `git send-email`).

| Piece | Upstream | List / maintainers (`scripts/get_maintainer.pl`) | Size | Difficulty |
|---|---|---|---|---|
| TPS68470 board data | `drivers/platform/x86/intel/int3472/tps68470_board_data.c` | platform-driver-x86@vger.kernel.org (Hans de Goede, Ilpo Järvinen) | ~100 lines | **easy** |
| ipu-bridge `INT347F` entry | `drivers/media/pci/intel/ipu-bridge.c` | linux-media@vger.kernel.org (Sakari Ailus) | 2 lines | **easy** (needs the sensor driver) |
| IMX241 driver | new `drivers/media/i2c/imx241.c` + Kconfig/Makefile + MAINTAINERS | linux-media (Sakari Ailus) | ~700 lines | **medium**: reviews are strict |
| DW9806B VCM | add to `dw9807-vcm.c` (i2c id + init table + regulators) or new driver | linux-media | ~60 lines | easy–medium |
| Speakers | new fixup in `sound/hda/codecs/realtek/alc269.c` | linux-sound@vger.kernel.org (Takashi Iwai) | ~150 lines | **medium** |
| libcamera helper | `src/ipa/libipa/camera_sensor_helper.cpp`, `camera_sensor_properties.cpp` | libcamera-devel@lists.libcamera.org | ~30 lines | **easy** |

## Speakers — what a kernel fix looks like
A quirk `SND_PCI_QUIRK(0x144d, 0xc135, "Samsung Galaxy TabPro S (SM-W700)", ALC298_FIXUP_SAMSUNG_MAX98925)`
whose fixup:
1. at `HDA_FIXUP_ACT_INIT`: writes the device-0x1b power-up sequence and configures both MAX98925s
   (gain/channel), using a new 8-bit bridge write helper (`0xb010`) next to the existing
   `alc298_samsung_write_coef_pack` (which uses `0xb011`);
2. registers a **playback hook** (like `alc298_samsung_v2_playback_hook`) that sets `GLOBAL_ENABLE`
   on stream open and clears it on close — this also fixes headphone muting and avoids depending on
   `power_save=0`, because the init is re-run after codec resume;
3. on jack events, mutes the amps when headphones are plugged in.

The upstream maintainers will ask what each 0x1b register means; sharing the emulation logs and the
MAX98925 identification helps. Testing on more units (and the Galaxy Book 12, which looks like the
same design) would strengthen the patch.

## Process (kernel)
1. Clone mainline (`git clone https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git`),
   apply the change in-tree, build, test on the tablet.
2. `./scripts/checkpatch.pl --strict`, write a commit message explaining the hardware, add
   `Signed-off-by:` (DCO — certifies you may submit it under the GPL).
3. `./scripts/get_maintainer.pl 0001-*.patch` → `git send-email` to those addresses.
4. Address review, resend `v2`, … Once accepted it lands in the next merge window and in Fedora
   ~2–4 months later. Board-data/quirk patches are often also backported to stable (`Cc: stable@`).

## Process (libcamera)
Same e-mail workflow against `https://git.libcamera.org/libcamera/libcamera.git`; small helper
patches are usually merged within weeks. A tuning file (`src/ipa/ipu3/data/imx241.yaml`) would be
a nice follow-up.

## Kernel bugzilla
Post the findings on [bug 188411](https://bugzilla.kernel.org/show_bug.cgi?id=188411) (needs a
bugzilla account) with a link to this repo — it is the reference that the next person finds.
