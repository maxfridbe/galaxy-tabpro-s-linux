# 07 – Hardware reference (SM-W700)

| | |
|---|---|
| DMI | `SAMSUNG ELECTRONICS CO., LTD.` / `Galaxy TabPro S` / board `SM-W700…`, family `SAMSUNG ATIV` |
| BIOS | AMI `P13HAB.000.161031.WY.1251` (2016-10-31), UEFI, Secure Boot |
| CPU | Intel Core m3-6Y30 (Skylake-Y), PCH Sunrise Point-LP |
| RAM / SSD | 4 GB / LITE-ON CV1-8B128 128 GB SATA M.2 (AHCI) |
| Wi-Fi / BT | Qualcomm Atheros QCA6174 (`168c:003e`) / QCA61x4 USB (`0cf3:e300`) |

## Audio
* HDA controller `8086:9d70` (Smart Sound Technology), NHLT table empty (no DSP DMIC endpoints).
* Codec **Realtek ALC298** `10ec:0298`, subsystem **`144d:c135`**, HDMI codec `8086:2809`.
* BIOS pin configs: `0x12 0x90a60130` (int. digital mic), `0x17 0x04211010` (HP jack),
  `0x18 0x04a11040` (headset mic jack), `0x1a 0x90170120` (internal speaker, line-level),
  others `0x411111f0`/unused.
* Bridge devices (Realtek coef bridge, 7-bit addresses): **0x1b** codec-internal I2S/HP block
  (ID `0x6311`), **0x31** MAX98925 left, **0x34** MAX98925 right.

## Touch / input
* Touchscreen: Synaptics I2C-HID `SYNP1234` (`_CID PNP0C50`), `\_SB.PCI0.I2C4.TPL1`, addr 0x20,
  HID descriptor reg 0x20, GpioInt level-low; I2C4 in ACPI mode (`INT3446`). Power via ACPI `_PS0`.
  Windows adds Samsung's `TchFilter` keyboard-class filter (not needed on Linux).
* GPIO buttons `INT33D2`, slate/laptop indicator `INT33D3`, dock indicator `INT33D4`, PS/2 kbd `MSF0001`.

## Sensors (untested on Linux)
`SMO8A80` K2HH accelerometer, `TOBH1733` BH1730 ambient light, `STH0001` SX931x proximity,
`SSE4752` Broadcom GNSS 4752 — all on `\_SB.PCI0.I2C?` (INT3447 = I2C controller).

## Cameras
See [04-camera.md](04-camera.md). Summary:
* CIO2 `8086:9d32`, ImgU `8086:1919`.
* TPS68470 `INT3472:04` (`\_SB.PCI0.CLP0`, "PMIC-CRDG"), I2C2 0x4d, REVID 0x21.
* Rear `INT347F:00` (`LNK0`, `NSMM4D5`): I2C2 0x10, VCM DW9806B 0x0c, EEPROM 0x58, CSI port 0, 2 lanes.
* Front `INT347F:01` (`LNK1`, `CM500RF05SW700`): I2C3 0x37, EEPROM 0x51, CSI port 1, 2 lanes.
* Camera AVStream (Windows) `INT3470`, `INT3471` (flash?) disabled.

## Other ACPI devices of note
`SAM0101` (Samsung AMOLED panel helper), `SAM0602` (Samsung "MLDR" loader), `SPNL0001`,
`SECM0001`, `USBC000` (Type-C), `INT33A1`/PEP for s2idle (LPIT present).
