# 06 – Reverse-engineering notes

How every magic number in this repository was obtained, so it can be checked, reproduced, or
applied to other machines (the Galaxy Book 12 has the same unsolved speaker bug).
No Windows binaries are included; you need your own copies (from the tablet's driver store via
[`windows-dump`](../windows-dump/), or Samsung's driver packages).

Tools (`re/`):
* `rtre.py` — loads a PE driver, splits it into functions via `.pdata`, disassembles with Capstone,
  indexes RIP-relative xrefs and call targets. `RTRE_SYS=driver.sys`.
* `emu.py` — Unicorn emulator for `RTKVHD64.sys` functions; stubs imports, turns every internal call
  into a no-op except the codec-bridge/coef primitives, which it logs.
* `emuskc.py` — same idea for `SkcController.sys`, but *constructs the real C++ objects*
  (allocations, vtables, a TPS68470 register file for read-modify-write).
* `hdav.py` — minimal `hda-verb` over `/dev/snd/hwC0D0` + the Realtek bridge protocol.
* `w700acpi.c` — kernel module that evaluates camera ACPI methods (`SSDB`, `_CRS`, `_DSM`s, `CLDB`)
  and prints them; needed because the reference ACPI code reads everything from NVS variables.
Python deps: `capstone pefile unicorn` (`python -m venv v && v/bin/pip install capstone pefile unicorn`).

## Speakers

1. **Elimination on Linux.** Headphones work (verified with a jack-detected tone) → HDA link,
   codec, DACs, routing OK. The speaker pin 0x1a is enabled/unmuted at full volume, so the
   problem is after the codec.
2. **Windows registry:** the Realtek device key holds `CaliDataL00…1f/R00…1f` (speaker calibration)
   and labels the speaker endpoint *RearLineOutWave3* → speakers are fed by an external smart amp.
3. **`RTKVHD64.sys` strings:** `SmartAMPCtrls`, `RTAMPCFG%d`, `DbgALC1006Cmd`, `DbgSAMPCmd`.
4. **Bridge protocol:** functions `0x47570`/`0x473a0` (read/write) and the object initialiser
   `0x45344`, which for codec id `0x298` maps the bridge to coefs `0x22` addr, `0x23` reg, `0x25`
   data, `0x26` cmd, `0x28` rdata, `0x89` reg-hi; command words `0xb012/0xb013/0xb033` (writes)
   and `0xb01a/0xb01b/0xb03b` (reads). `0x464cc`/`0x4664c` are the 8-bit variants (`0xb010`/`0xb018`).
5. **Emulation** of `0x39daec` (init + calibration), `0x39ee78`/`0x39f304` (playback power-up) and
   `0x39f7b4` (power-down) → exact register sequences for bridge device **0x1b**
   ([`re/data/emu-39daec.txt`](../re/data/emu-39daec.txt), [`emu-others.txt`](../re/data/emu-others.txt)).
   Device 0x1b reads back ID `0x6311` and the very values Windows writes → it is the codec's internal
   RT56xx-style block.
6. **Bus scan** (read-only) of all bridge addresses → devices at **0x31** and **0x34** (8-bit regs,
   0x00–0x3E, `0xFF=0x80`), which match the **MAX98925** register map/defaults in Linux's
   `sound/soc/codecs/max98925.{c,h}` (removed upstream in later kernels; fetch from v4.19).
7. Their state: configured by firmware, `GLOBAL_ENABLE` (0x38) = 0. Setting 0x38=0x80 alone: silent;
   after replaying the 0x1b power-up: **sound**.

## Cameras

1. **Windows device dump:** both cameras are `ACPI\INT347F` with service `IMX241`; the PMIC is
   `INT3472` (*Intel Control Logic*, `SkcController.sys`).
2. **Linux dmesg:** `int3472-tps68470 … No board-data found for this model`.
3. **ACPI** (`re/w700acpi.c` → [`re/data/acpidump.txt`](../re/data/acpidump.txt)): SSDB lanes=2, link
   0/1, MCLK 19.2 MHz, `_PLD` back/front, `_CRS` I2C addresses, `_DSM 5815c5c8…` I2C device table
   (sensor/VCM/EEPROM), VCM type 6 = DW9806B, PMIC at I2C2 0x4d, `CLDB` control-logic type 2 = TPS68470.
4. **Sensor registers** from `imx241.sys` `.rdata`: arrays of 16-byte `{width, reg, value, delay}`
   entries terminated by `0xffff` — four mode tables (63 entries each) plus reset/stream-on/off/hold
   tables ([`re/data/imx241-tables.txt`](../re/data/imx241-tables.txt)). Standard SMIA layout; PLL
   `0x0305=2, 0x0307=0x68` with 19.2 MHz → 998.4 Mbps/lane → 499.2 MHz link frequency.
5. **PMIC power-up** (`SkcController.sys`): classes `tps68470::SSTps68470`, `SSCrdG2TiSensor`,
   `SSTps68470VoltageWF/UF` (world/user facing). `SensorOn(link, cfg)` with `cfg[0]` = 5 (WF) / 4 (UF)
   and five rail voltages in millivolts at `cfg+8`, which come from `imx241.sys`
   (`mov dword [rbx+0xa8a], 0x7080af0` …): core 1200 (0 for module FN50FF469H), analog 2800,
   VIO 1800, VCM 2800 (only with VCM), VSIO 1800. `Initialize` converts them to register codes with
   the TPS68470 LDO formula. Emulated output: [`re/data/tps68470-poweron.txt`](../re/data/tps68470-poweron.txt).
6. **DW9806B:** `imx241.sys` `DW9806B_Init` writes the table at `0x14001a2f8`:
   `02=01, 02=00, delay 1 ms, 02=02, 06=61, 07=13`; position is 10-bit at reg 0x03 (as DW9807).
7. **Verification:** chip id `0x0241` on both sensors; raw RAW10 capture → recognisable scene with
   correct colours (confirms RGGB); libcamera capture at ~36 fps; AE drives exposure/gain controls.

## Pitfalls hit (so you don't)
* HDA hwdep verbs use `HDA_VERB(nid, verb, param) = nid<<24 | verb<<8 | param`, not `nid<<20`.
* hwdep needs `CAP_SYS_RAWIO` (root) even with the seat ACL.
* Codec runtime suspend wipes the 0x1b block config → keep `power_save=0` (and fight tuned).
* WDF drivers call through a function table filled at load time → in emulation map a `ret 0` stub at 0.
* `SensorOn` overwrites the voltage config through its vtable first — pass voltages there.
* On Silverblue, `insmod` from `/usr/local` is denied by SELinux unless labelled `modules_object_t`,
  and the initramfs already loaded the stock PMIC module.
* MokManager needs a physical keyboard and times out quickly.

## Screen brightness
See [12-brightness.md](12-brightness.md): `SAMOPanel.sys` interface table via `\Callback\IGD_PanelDriverShareObj`, emulated with `re/emusamo.py`, AUX accesses forwarded live to the panel to capture its calibration.
