# windows-dump/ – back up the Windows hardware view before wiping it

A single static Rust executable (`w700dump.exe`). Put it on a USB stick, double-click it in Windows,
accept UAC. It writes `w700-backup\<PC>-<date>\` next to itself (~250 MB):

| output | |
|---|---|
| `devices.json` | every SetupAPI device (present + ghost) with **all** device properties |
| `device-tree.txt` | live PnP tree with problem codes |
| `summary.txt` | audio / touch / HID / sensor / ACPI / problem devices, human readable |
| `acpi/*.bin`, `smbios.bin` | firmware tables (only the first SSDT of each kind — Windows API limit; the registry export has the DSDT) |
| `reg/*.reg` | Enum, Class, DeviceClasses, Services, HARDWARE, MMDevices, Realtek, Samsung … |
| `cmd/*` | pnputil, driverquery, systeminfo, msinfo32, dxdiag, powercfg, bcdedit, CIM queries, setupapi logs |
| `drivers/oemN/` | exported third-party drivers for audio/HID/touch/sensor/ACPI/problem devices |

**Privacy:** the backup contains your registry, product IDs and the Windows licence table — do not
publish it.

Build (from Linux): `rustup target add x86_64-pc-windows-gnu` (+ `mingw64-gcc`), then
`cargo build --release --target x86_64-pc-windows-gnu`.
