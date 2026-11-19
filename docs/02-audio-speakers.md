# 02 – Internal speakers

## TL;DR
```
sudo ./audio/install.sh
```
Speakers work immediately, after every boot and after resume. Uninstall with
`sudo ./audio/install.sh --uninstall`.

## Symptoms (before the fix)
* Headphones work, speakers are completely silent.
* `aplay -l` / PipeWire show *Built-in Audio Analog Stereo* with a *Speakers* port; volume and
  mute look correct; the codec's speaker pin (node `0x1a`) is enabled and unmuted.
* None of the kernel's Samsung amp quirks help (`model=alc298-samsung-amp`, `-v2-2-amps`, `-v2-4-amps`).

This has been the state since the tablet came out
([kernel bug 188411](https://bugzilla.kernel.org/show_bug.cgi?id=188411), Mint/FydeOS forum threads).

## The hardware
```
 HDA link          Realtek ALC298 codec (10ec:0298, subsystem 144d:c135)
 ──────────► ┌──────────────────────────────────────────────┐
             │ DACs → mixers → pins 0x17 (headphone jack) ──┼──► 3.5 mm jack   (works)
             │                       0x1a ("speaker", line) │
             │ internal block @ bridge address 0x1b         │
             │   (RT56xx-like: I2S/clock/power registers) ──┼──► I2S ──┐
             │ vendor node 0x20: coef bridge to the bus  ───┼──► "I2C" ┤
             └──────────────────────────────────────────────┘         │
                                           ┌──────────────────────────┴─────────┐
                                           │ Maxim MAX98925 @0x31 → left speaker │
                                           │ Maxim MAX98925 @0x34 → right speaker│
                                           └─────────────────────────────────────┘
```
* The speakers are driven by **two Maxim MAX98925 smart amplifiers**. They are *digital-input*
  (I2S) amps, sitting behind the ALC298's internal **coefficient-register bridge**, at 7-bit addresses
  **0x31** (left) and **0x34** (right). They are identified by their revision register `0xFF = 0x80`
  and by register defaults matching the Linux `max98925` driver's table exactly.
* Nothing in ACPI describes them; on Windows the Realtek driver (`RTKVHD64.sys`) handles everything.

### Why the existing kernel quirks don't work
The kernel's Samsung fixups (`alc298_samsung_amp*` in `sound/hda/codecs/realtek/alc269.c`) talk to
amps at **0x38–0x3d** using bridge command **`0xb011`** (16-bit register address, 8-bit data). Here the
amps are at 0x31/0x34 and use **8-bit registers** (`0xb010` write / `0xb018` read). They never reach them.

### Why the speakers are silent
1. The MAX98925s come out of reset already configured by the firmware (I2S format, clocks, gain,
   boost + speaker blocks enabled) **but `GLOBAL_ENABLE` (reg 0x38) is 0** — nothing turns them on.
2. The ALC298's internal block at bridge address **0x1b** (the I2S source for the amps) has to be put
   in its playback power state; Linux never does that.

## The fix (`audio/w700-speakers`)
A ~100-line Python script, no dependencies, run as root at boot and after resume:

1. Checks it is on this exact codec (vendor `10ec0298`, subsystem `144dc135`) and that a MAX98925
   answers at 0x31 — otherwise it does nothing.
2. Replays the Windows playback power-up of device `0x1b` (28 register writes extracted from
   `RTKVHD64.sys` function `0x39ee78` by emulation — see [06](06-reverse-engineering.md)).
3. For each amp: clears latched IRQ flags, sets **reg 0x2D** = channel select (left/right I2S slot)
   + speaker gain, sets **reg 0x38 = 0x80** (GLOBAL_ENABLE).

It talks to the codec through the HDA hwdep device (`/dev/snd/hwC0D0`, `HDA_IOCTL_VERB_WRITE`) using
the Realtek coef registers on node 0x20:

| coef | meaning |
|---|---|
| `0x22` | bridge device address (7-bit) |
| `0x23` | register (low 16 bits) |
| `0x89` | register high bits (32-bit transactions) |
| `0x25` | write data |
| `0x26` | command / status (bit 14 = busy). Write cmds `0xb010` 8/8-bit, `0xb012` 8-bit reg/16-bit data, `0xb013` 16/16, `0xb033` 32-bit reg; reads are the same `| 0x08` (`0xb018`, `0xb01a`, `0xb01b`, `0xb03b`) |
| `0x28` | read data |

### Codec power saving must be off
If the codec runtime-suspends, the 0x1b block loses its configuration and the speakers go silent
again. `install.sh` therefore sets `snd_hda_intel power_save=0 power_save_controller=N`
(`/etc/modprobe.d/w700-audio.conf`) and overrides `tuned`'s `balanced`/`powersave` profiles, which
otherwise set `[audio] timeout=10` (`/etc/tuned/profiles/*/tuned.conf`). Cost: a little battery.

### Loudness (`/etc/w700-speakers.conf`)
`GAIN=<0..31>`, dB = GAIN − 6. The firmware default is 25 (+19 dB); we ship **28** (+22 dB), the
maximum is 31. **Windows protects these speakers with Realtek's speaker-protection DSP; Linux has
none** — raise carefully. Apply with `sudo /usr/local/sbin/w700-speakers` (or reboot).

### Stereo
Register 0x2D bits 6:5 select the I2S input: the firmware leaves both amps on *(L+R)/2 mono*. The
script sets 0x31 → left slot, 0x34 → right slot (verified by ear with a left/right test clip).

## Verifying / debugging
```
journalctl -u w700-speakers -u w700-speakers-resume      # "enabled 0x31:en=0x80,gain=0x1c 0x34:..."
sudo python3 re/ident.py                                 # dump amp registers (needs re/hdav.py)
```
`re/hdav.py` is a tiny `hda-verb` replacement with the bridge protocol implemented (`br`/`bw`,
`r8`/`w8`); `re/scan.py` scans all bridge addresses (read-only).

## Known issues
* Plugging headphones in does **not** mute the speakers yet (the amps stay enabled; the HDA driver
  only mutes the analog speaker pin). TODO: hook jack events and toggle reg 0x38.
* Proper fix belongs in the kernel — see [09-upstreaming.md](09-upstreaming.md).
