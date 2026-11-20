# 13 – How this was done: methods for getting hardware working and testing it

This page describes *how* the fixes in this repository were found and verified, so the same
approach can be reused on other machines. It complements [06](06-reverse-engineering.md) (where each
magic number came from) with the general workflow, the testing methods, and the mistakes that
taught us something.

## Ground rules
* **No Windows binaries or firmware in the repository.** Tools take the user's own copies
  (from the tablet's driver store, backed up with [`windows-dump/`](../windows-dump/)). Only
  hardware facts end up here: register addresses, values and sequences.
* **Read before write.** Every new register interface is first probed read-only (bus scans,
  register dumps). Writes are limited to registers Windows is known to write, and every
  experiment records the old value and restores it.
* **Log everything that touches hardware.** Each tool logs its writes, and snapshots of the full
  device state are taken before and after experiments (see *State snapshots*).
* **One variable at a time.** When something starts working, find out *which* change did it
  before writing the fix (see the speaker A/B tests below).

## 1. Collect
| What | How |
|---|---|
| Windows device tree, drivers, registry, ACPI | [`windows-dump/`](../windows-dump/): run from a USB stick *before* installing Linux |
| Linux view of the same hardware | `dmesg`, `/proc/asound/card0/codec#0`, `/sys/bus/*/devices`, `acpidump` ([`installer/linux-collect.sh`](../installer/)) |
| ACPI methods that read NVS variables | [`re/w700acpi.c`](../re/w700acpi.c), a kernel module that evaluates them on the live machine |

Comparing the two views quickly shows which devices have no Linux driver (cameras, sensors,
GNSS) and which are driven but silent (speakers, brightness).

## 2. Static analysis of the Windows driver
[`re/rtre.py`](../re/rtre.py) splits a driver into functions using `.pdata`, disassembles them with
Capstone and indexes calls and RIP-relative references. Useful patterns:
* **Find the hardware primitives first** (e.g. the Realtek codec-bridge read/write functions or a
  panel's AUX transfer), then list every caller. Callers with constant arguments are often
  complete init tables.
* **Per-model switches.** Drivers that support many laptops branch on the codec subsystem ID.
  Searching for the machine's subsystem (`0xc135`) leads straight to its configuration branch.
  For example, `RTKVHD64.sys` at `0x38ea6c` sets "bridge device 0x1b, type 2" for this tablet.
* **Vtables and object factories.** Functions with no direct callers are usually reached through
  a vtable. Finding the vtable, then the constructor, then the factory shows the conditions under
  which a code path is used at all. That is how the PCI-ID condition below was found.
* Strings and pool tags help (`'Sams'` was the pool tag of the Samsung amp class).

Static analysis is good at finding *where* things happen. On its own it rarely tells you *what
actually runs* on a given machine.

## 3. Emulation with live hardware behind it
[`re/emu.py`](../re/emu.py), [`re/emuskc.py`](../re/emuskc.py) and
[`re/emusamo.py`](../re/emusamo.py) run single driver functions in Unicorn:
* Imports are stubbed. Internal calls become no-ops, except for the hardware primitives, which are
  logged or forwarded.
* With **live forwarding**, the emulated function's hardware accesses go to the real device. The
  AMOLED panel is an example: [`re/auxsrv.py`](../re/auxsrv.py) executes DP AUX transfers on
  `/dev/drm_dp_aux0`. The driver code then reads the panel's real calibration and computes real
  values, which ended up in the per-panel brightness profile.
* Emulation works well for functions with simple inputs (TPS68470 power-up, codec power
  sequences, the panel gamma interface). It breaks down when the right behaviour depends on a lot
  of driver state built up elsewhere. Then the next method is needed.

## 4. The whole Windows driver in a VM, with the real chip behind it
Used for the speakers, where emulating single functions kept missing the essential step.
[`re/vm-verb-forward/`](../re/vm-verb-forward/):
* A QEMU HD Audio codec device (`hda-forward`) sends every verb from the guest to a helper on the
  tablet ([`hda-verb-server.py`](../re/vm-verb-forward/hda-verb-server.py)). The helper executes the
  verb on the real ALC298 through `/dev/snd/hwC0D0` and returns the real response. To Windows the
  real chip appears to sit inside the VM.
* The guest's audio stream is consumed at the real rate, so playback states behave normally.
* The Windows driver INF is retargeted from `INTELAUDIO\…` to `HDAUDIO\…`, and installed with
  driver signature enforcement disabled for one boot.
* **The driver must believe it runs on the real platform.** Here, the Samsung amp code only runs
  when the HD Audio controller reports Intel Sunrise Point (8086:9D70). With QEMU's default ICH9 ID
  the driver loaded fine but never touched the amps. Finding that took the static analysis in §2.
* The helper logs every verb. [`decode.py`](../re/vm-verb-forward/decode.py) turns the log into
  coefficient and bridge transactions:
  [`re/data/windows-driver-start.txt`](../re/data/windows-driver-start.txt).

The VM ran on a desktop and reached the tablet over an SSH tunnel; 10 ms per verb is fast enough.
The same approach should work for any HDA codec quirk where Windows works and Linux doesn't.

## 5. Talking to the hardware from Linux
* **HDA codec:** HDA hwdep ioctls (`HDA_IOCTL_VERB_WRITE`) from Python. The Realtek
  coefficient bridge (coefs 0x22–0x28/0x89) reaches devices behind the codec.
  [`re/hdav.py`](../re/hdav.py), [`re/scan.py`](../re/scan.py), [`re/ident.py`](../re/ident.py).
* **eDP panel:** `/dev/drm_dp_aux0` (DPCD), including the vendor unlock for the Parade TCON.
* **I2C sensors / PMIC:** kernel drivers built out of tree (`camera/kernel`, `sensors/kernel`).
* **UART GNSS:** a small serdev driver plus a user-space PE loader, so Broadcom's own engine runs
  natively ([`gnss/`](../gnss/)).

## 6. State snapshots and diffs
A snapshot script dumps everything relevant to JSON: codec coefficients, the codec block's
registers, both amps' registers, GPIO and pin controls. Comparing snapshots is the most effective
debugging tool here:
* **broken vs working:** after the speakers worked again, diffing against the post-power-loss
  snapshot gave the complete list of candidate differences;
* **A/B elimination:** each group of differences was put back to its broken value, and only the
  amp table turned out to matter; codec coefficients and the codec block's state didn't;
* **reproducing the failure:** the final fix was tested by writing the *broken* amp values back
  (the post-power-loss state), then running only the new script.

## 7. Testing
### Acoustic loopback (speakers, microphone)
The tablet tests itself: play a 1 kHz tone through the speakers, record it with the internal
microphone, and measure the 1 kHz amplitude (Goertzel / single-bin DFT). Compare it with the same
bin in a stretch of the recording *before* the tone starts.
* A working speaker gives roughly **36 000** against a noise floor of **2–40**. There is no grey zone.
* Left-only and right-only tones check the stereo assignment.
* Caveats learned the hard way:
  * Compare against the pre-tone window, not a fixed threshold; room noise varies.
  * Check that capture is working at all: a stuck DMA pointer (`/proc/asound/card0/pcm0c/sub0/status`
    `hw_ptr 0`) gives a silent recording that looks like "speakers off".
  * Small, inconsistent readings (10–100) are room noise, not partial success; repeat any
    positive result before believing it.

### Ears, with labelled notifications
For anything a microphone can't judge (distortion, which side, loudness), a test script shows a
desktop notification ("Test 3", "Beep now") right before each sound, and the person reports which
numbers they heard. Check that the test actually ran (its log or journal) before trusting a
"nothing": one run failed to start because SELinux blocked executing a script from `/tmp`.

### Hardware self-diagnostics
Devices often report their own state: MAX98925 status/flag registers, DPCD read-back of the panel
unlock, light-sensor raw counts, the GNSS engine's satellite and HDOP figures. Deliberately
provoking a fault flag (for example a TDM slot conflict, which needs a running bus clock) can serve as a
detector. **Validate such a detector with a known-good case first;** an unvalidated detector only
gives negative results, and those mean nothing.

### Sensors and GNSS
* Light sensor: lux readings checked against the Windows ACPI calibration and the Windows
  brightness curve.
* GNSS: fix quality, satellites and accuracy logged by the program; GPS-only vs GPS+GLONASS
  positions compared for consistency. Positions are only logged at debug level (privacy).

### Packages
Every component is built in a Fedora container (the same build as CI), installed on the tablet,
and checked again **after a reboot**, because many failures only appear at boot (module signing,
SELinux labels, service ordering).

## 8. Lessons
* **"Works" can depend on state left behind by Windows.** The first speaker fix only enabled the
  amps. It survived every reboot because Windows had configured the amps before Linux was installed. A
  flat battery cleared the amps and exposed the gap. Test fixes from a cold, reset state when
  possible: reproduce the reset state (write power-on values back), or pull the power.
* **Look at what Windows does *not* do.** During speaker playback Windows keeps the codec's
  internal block powered down. The earlier "required" power-up was never needed.
* **The environment must match the real platform.** IDs reported by the bus or controller can
  silently switch whole code paths off.
* **rpm-ostree stages updates at shutdown.** A deployment staged just before a power loss is lost,
  so stage again after an unexpected power-off.
* **Exclusive interfaces:** the HDA hwdep device can be open by only one tool at a time. A guest
  driver that reprograms converter streams leaves Linux audio confused until `snd_hda_intel` is
  re-bound.
