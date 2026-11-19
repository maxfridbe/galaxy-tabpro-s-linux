# 12 – Screen brightness (AMOLED)

**Status:** ✅ works with [`brightness/`](../brightness/). GNOME's slider and the brightness keys
work, using a per-panel profile generated once with [`re/samo-profile.py`](../re/samo-profile.py).

## Symptom
The brightness slider moves, `/sys/class/backlight/intel_backlight/brightness` changes, and the
screen stays exactly as bright. Other TabPro S owners have reported the same thing
([infernix/samsung_tabpro_s](https://github.com/infernix/samsung_tabpro_s), KDE bug 525717). Until
now the only workaround was software dimming (`xrandr --brightness`, GNOME extensions).

## The panel
| | |
|---|---|
| Panel | Samsung Display **SDC A029** (EDID, 2015, 2160×1440 AMOLED) |
| Timing controller | **Parade** eDP TCON: DPCD sink OUI `00-1C-F8` at `0x400` |
| eDP DPCD `0x701` / `0x702` | `0x8a` / `0xef`: AUX brightness advertised, but `TCON_BACKLIGHT_ADJUSTMENT_CAP` is missing |
| Windows driver | `SAMOPanel.sys` "Samsung AMOLED Panel", ACPI `SAM0101` (`_SUB C135144D`) |

An OLED has no backlight. Each of the three "standard" controls does nothing:

* **PWM** (`intel_backlight`, max 937): the pin goes nowhere useful.
* **VESA eDP AUX brightness** (`0x720` enable bit, `0x721=0x02` DPCD mode, `0x722/0x723` level): the
  registers accept writes, and the panel ignores them, tested at several levels.
* Software dimming works, but it only scales the pixel values and keeps the panel's peak luminance.

## How Windows does it
`SAMOPanel.sys` is a small KMDF driver. It opens the callback object
`\Callback\IGD_PanelDriverShareObj`, through which Intel's graphics driver and Samsung's driver
exchange function tables. Samsung's table has 7 entries, and the large one (`0x140006110`) handles
panel events: init, power, brightness. Intel's side hands Samsung a single "AUX transfer" entry
point (`ctx+0x298`, command `0x28`, `{read=0/write=1, {dpcd address, buffer, length}}`).

Everything goes through three vendor DPCD registers of the Parade TCON:

| DPCD | Meaning |
|---|---|
| `0x490` | **unlock**: write the ASCII bytes `P A R A A U X - R E G` one at a time; reads back `01` when unlocked |
| `0x491` | bank |
| `0x492` | register inside the bank |
| `0x493` | data (read/write the selected register) |

A multi-byte AUX write starting at `0x491` sets bank, register and data in one transaction.
**Without the unlock, the bank and register selectors still latch but every data write is ignored
and every read returns `00`.** That's the trap that made all early experiments fail.

What the driver does with the window:

| Bank / reg | Use |
|---|---|
| `0x02/0x7f`, `0x07/0xff`, `0x0f/0xff` | sub-page selects / access enables |
| `0x07/0x00-0x02`, `0x07/0x03`, `0x07/0x06`, read `0x07/0x0a`, `0x07/0x0c` | read the panel's **factory calibration** (MTP/flash, 87 reads, addresses like `0x020000`, `0x028000`) |
| `0x0d/0x00-0x75` | 118-byte table computed from the calibration (init) |
| `0x07/0x10-0x30` + trigger `0x07/0x32=1` | a **33-byte panel command**, the size of a Samsung AMOLED gamma command |
| `0x09/0xf0, 0xf8, 0xf9` | brightness level: `0xf9` runs from `0x04` (0 %) to `0xf9` (100 %) |
| `0x0e/0x10-0x18`, `0x0e/0x80`, `0x0f/0x9b`, `0x0f/0x9d`, `0x0f/0x30` | level-dependent luminance/gamma tuning |

Brightness on this panel is real AMOLED drive-level control: gamma and luminance tables, computed by
Samsung's floating-point code (tables of luminance curves in the driver) from the panel's own
calibration. A brightness change is 54 AUX writes. The init is about 1,500 writes plus 87 reads.

## How we reproduce it on Linux
We can't ship Samsung's driver or re-implement its gamma maths (yet), so:

1. [`re/samo-profile.py`](../re/samo-profile.py) runs the init function of *your* `SAMOPanel.sys`
   in a Unicorn emulator ([`re/emusamo.py`](../re/emusamo.py)) and forwards its AUX accesses live to
   the panel ([`re/auxsrv.py`](../re/auxsrv.py), `/dev/drm_dp_aux0`). The panel therefore returns its
   real calibration.
2. It re-runs the same code offline with the recorded reads and records the writes for every
   level 0…100. Each level is a pure list of writes (no reads), and regenerating gives a
   byte-identical profile.
3. [`brightness/tabpro-s-brightness`](../brightness/tabpro-s-brightness) (a daemon, Python, no
   dependencies) unlocks the TCON, replays the init, and follows
   `/sys/class/backlight/intel_backlight/brightness` (inotify). GNOME, logind and the brightness keys
   keep driving `intel_backlight` as usual. Every 3 s it checks `0x490`; if the TCON was reset
   (suspend, panel power-off) it unlocks and re-applies.

The profile depends on the panel's factory calibration, so **generate it on your own tablet**:
```
toolbox create re && toolbox run -c re pip install --user capstone pefile unicorn
# copy SAMOPanel.sys from your Windows install (C:\Windows\System32\drivers\SAMOPanel.sys)
# or from Samsung's driver package
RTRE_SYS=SAMOPanel.sys toolbox run -c re python3 re/samo-profile.py -o amoled-profile.json
sudo install -Dm0644 amoled-profile.json /etc/tabpro-s/amoled-profile.json
rpm-ostree install tabpro-s-brightness && systemctl reboot      # from the package repo (docs/11)
```
Without the package: `sudo install -m0755 brightness/tabpro-s-brightness /usr/local/sbin/`,
`sudo install -m0644 brightness/tabpro-s-brightness.service /etc/systemd/system/`,
`sudo systemctl enable --now tabpro-s-brightness`. The service only starts when the profile exists.
One-shot test without the service: `sudo /usr/libexec/tabpro-s/brightness 30` (package) or `sudo tabpro-s-brightness 30`.

Notes:
* Panel Self Refresh (PSR1, the i915 default) can stay on; brightness works with it.
* Windows' INF sets `PSREnable=0`, so Windows runs with PSR off. We haven't seen a problem with it on.
* Windows driver version used: `SAMOPanel.sys` 16.1.1.7 (2016-04-02).

## Next steps
* Port the gamma/luminance calculation (`0x140003a48` and helpers) to plain code, so that no Windows
  file is needed and the profile can be computed from the 87 calibration bytes at boot.
* A proper kernel solution: a panel-specific backlight in i915 (`intel_dp_aux_backlight.c` already
  has vendor-specific paths), registered as the `raw`/`firmware` backlight instead of the PWM one.
