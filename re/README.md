# re/ – reverse-engineering tools and extracted data

See [docs/06-reverse-engineering.md](../docs/06-reverse-engineering.md) for the full story.

| file | |
|---|---|
| `rtre.py` | PE driver function index + disassembly helper (Capstone/pefile). `RTRE_SYS=x.sys` |
| `emu.py` | Unicorn emulation of `RTKVHD64.sys` functions, logs codec-bridge traffic |
| `emuskc.py` | emulation of `SkcController.sys` Samsung TPS68470 power-on |
| `hdav.py` | HDA verb + Realtek bridge protocol over `/dev/snd/hwC0D0` (root) |
| `scan.py`, `probe.py`, `ident.py`, `dump8.py` | bridge bus scan / register dumps (read-only) |
| `pins.py`, `stereo.py` | pin routing / amp channel experiments |
| `micsweep.py`, `miccheck.py`, `micvol.py` | speaker→mic loopback measurements |
| `emusamo.py` | emulation of `SAMOPanel.sys` (AMOLED panel driver); can forward AUX accesses to the real panel |
| `samo-profile.py`, `auxsrv.py` | generate the per-panel brightness profile (docs/12) |
| `regbin.py` | extract ACPI tables from a `HKLM\HARDWARE` `.reg` export |
| `w700acpi.c` | kernel module printing camera ACPI method results |
| `data/imx241-tables.txt` | IMX241 register tables from `imx241.sys` |
| `data/tps68470-poweron.txt` | emulated PMIC power-on writes (rear, front) |
| `data/acpidump.txt` | evaluated `LNK0/LNK1/CLP0` ACPI methods |
| `data/seq-1b-powerup.txt`, `data/emu-*.txt` | emulated bridge sequences from `RTKVHD64.sys` |

No Windows binaries are included. `pip install capstone pefile unicorn`.
