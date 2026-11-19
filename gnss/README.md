# gnss/ – Broadcom BCM4752 GPS/GNSS

**Status:** ✅ works. A warm start gets a fix in about 40–90 s (indoors near a window: 5–9 satellites,
GPS + GLONASS, about 10 m). Positions reach GeoClue, so GNOME Maps, Weather and the clock's
location use the GPS.

The TabPro S has a Broadcom **BCM4752** GNSS receiver (ACPI `SSE4752`, `\_SB.GPS0`) on the LPSS UART
`ttyS4`. The UART runs at 115 200 baud at power-up and uses hardware flow control. One GPIO (`RPIN`,
"REGPU") enables power.

The chip has no positioning firmware of its own. All position computation happens on the host in
Broadcom's closed **GLL engine** (`BcmGnss.dll`, v19.13 / GLL 17.19.13). The engine speaks a binary
protocol to the chip. Nobody has an open replacement, so this directory runs Broadcom's own engine
on Linux, without Wine.

## Pieces
| | |
|---|---|
| [`kernel/bcm4752`](kernel/bcm4752/bcm4752.c) | serdev + GNSS-subsystem driver: `/dev/gnssN` raw pass-through, hardware flow control, sysfs `speed` (UART baud, set by the engine) and `regpu` (power GPIO) |
| [`tabpro-s-gnss/`](tabpro-s-gnss/) | Rust program that **loads `BcmGnss.dll` natively** (see "How it works" below) |
| [`bin/tabpro-s-gnss`](bin/) | prebuilt x86_64 binary of the above (Fedora, glibc ≥ 2.34), checksum in `bin/tabpro-s-gnss.sha256`, rebuilt by [`build-binary.sh`](build-binary.sh) |
| [`systemd/tabpro-s-gnss.service`](systemd/tabpro-s-gnss.service) | boot service: waits for the driver, creates `gpsconfig.xml` on first start, runs the engine |
| [`geoclue/90-tabpro-s-gnss.conf`](geoclue/90-tabpro-s-gnss.conf) | tells GeoClue to read NMEA from `/run/tabpro-s-gnss/nmea.sock` |
| [`selinux/tabpro_s_gnss.te`](selinux/tabpro_s_gnss.te) | lets GeoClue (`geoclue_t`) connect to that socket |

## What you need from Windows
**`BcmGnss.dll`** and **`BcmGnssConfig.xml`**. They are Broadcom's and are **not redistributed
here**. Both are in the tablet's own Windows driver folder:
```
C:\Windows\System32\DriverStore\FileRepository\bcmgnsslocationsensor.inf_amd64_*\
```
Get them from the Windows partition before wiping it, or from a backup made with
[`windows-dump/`](../windows-dump/): look in `drivers\oemN\` for the folder with `BcmGnss.dll`. The GPIO helper DLL from the same
folder is **not** needed; `tabpro-s-gnss` has its own built-in replacement.

Put both files in `/var/lib/tabpro-s-gnss/`:
```
sudo install -d /var/lib/tabpro-s-gnss
sudo cp BcmGnss.dll BcmGnssConfig.xml /var/lib/tabpro-s-gnss/
```
On the first start, the service turns `BcmGnssConfig.xml` into `gpsconfig.xml`. It replaces the
`<hal>` element with Linux settings (`tabpro-s-gnss --make-config`). Delete `gpsconfig.xml` to
regenerate it. Until `BcmGnss.dll` is present, the service just doesn't start.

## Install

### A. RPM packages (recommended)
`tabpro-s-camera-kmods` carries the `bcm4752` module, and `tabpro-s-camera.service` loads it at
boot (signed locally under Secure Boot, like the camera modules). `tabpro-s-gnss` carries the
program, the service, the GeoClue configuration and the SELinux module.
```
rpm-ostree install tabpro-s-camera tabpro-s-camera-kmods tabpro-s-gnss    # dnf install on Workstation
# copy BcmGnss.dll + BcmGnssConfig.xml to /var/lib/tabpro-s-gnss/ (above), then
systemctl reboot
```
The `tabpro-s-gnss` RPM is built from source by CI ([`packaging/specs/tabpro-s-gnss.spec`](../packaging/specs/tabpro-s-gnss.spec)).

### B. Prebuilt binary, by hand
```
git clone https://github.com/maxfridbe/galaxy-tabpro-s-linux && cd galaxy-tabpro-s-linux/gnss
sha256sum -c bin/tabpro-s-gnss.sha256
make -C kernel                                   # bcm4752.ko for the running kernel (kernel-devel)
make -C kernel sign MOK_KEY=... MOK_CERT=...     # only with Secure Boot
sudo insmod kernel/bcm4752/bcm4752.ko            # or let tabpro-s-camera.service load it
sudo install -m 0755 bin/tabpro-s-gnss /usr/local/bin/tabpro-s-gnss
sed 's#/usr/bin/tabpro-s-gnss#/usr/local/bin/tabpro-s-gnss#g' systemd/tabpro-s-gnss.service | \
  sudo tee /etc/systemd/system/tabpro-s-gnss.service
sudo install -D -m 0644 geoclue/90-tabpro-s-gnss.conf /etc/geoclue/conf.d/90-tabpro-s-gnss.conf
checkmodule -M -m -o /tmp/t.mod selinux/tabpro_s_gnss.te && \
  semodule_package -o /tmp/t.pp -m /tmp/t.mod && sudo semodule -i /tmp/t.pp
sudo systemctl daemon-reload && sudo systemctl enable --now tabpro-s-gnss && sudo systemctl restart geoclue
```

### C. From source
```
cargo build --release --manifest-path tabpro-s-gnss/Cargo.toml   # → tabpro-s-gnss/target/release/
gnss/build-binary.sh                                             # or: rebuild bin/ in a Fedora container
```
Only the `libc` crate is needed. Then continue as in B.

## Checking it
```
systemctl status tabpro-s-gnss
journalctl -u tabpro-s-gnss -f        # "fix quality 1 satellites 07 hdop 1.2" once a minute
nc -U /run/tabpro-s-gnss/nmea.sock    # raw $GPGGA / $GPRMC (contains your position)
```
Run it in the foreground with `sudo systemctl stop tabpro-s-gnss; sudo tabpro-s-gnss -vv`. Use
`-vvv` to also log every byte on the UART and each raw location report.

**Privacy:** below `-vvv`, the log shows only fix quality, satellite count and HDOP, never coordinates.

## How it works
* **PE loader** ([`src/pe.rs`](tabpro-s-gnss/src/pe.rs)). Maps the DLL at its preferred base,
  applies relocations, and resolves imports by name or ordinal against our own implementations.
  Missing functions become stubs that abort with their name.
* **Windows API** (`k32.rs`, `other.rs`, about 200 functions, all `extern "win64"`):
  * threads, events, critical sections, TLS, and alertable waits with APCs;
  * overlapped I/O that completes the Windows way;
  * files under the root directory, time.
  * A fake TEB in `GS` (via `arch_prctl`) so the MSVC code's `gs:` accesses work.
  * Registry, COM, USER32 and WinHTTP are stubs.
* **Serial port** (`comm.rs`). `BCMGNSS1` maps to `/dev/gnssN`. Baud changes go to the driver's
  sysfs `speed`; the engine switches to 921 600. Reads and writes are queued strictly in order, as
  a Windows serial driver does.
* **GPIO helper DLL** (built in). Pin 1 = REGPU via sysfs `regpu`; pin 0 = RESET (not wired).
* **Engine.** The program runs `glgps_main(0, NULL, "gpsconfig.xml", 0)` in a thread, as
  `BcmGnssLocationSensor.dll` does. It then posts
  `$pglirm,req_pos,win_xp_immediate_periodic_request,acc,50`.
  * Diagnostics arrive through `GpsHalSetDriverTraceFuncPtr`.
  * Positions arrive through `GpsHalSetLocSensorFuncPtr` as a 0x910-byte report (layout below).
* **NMEA** (`nmea.rs`). Each report becomes `$GPGGA` + `$GPRMC` on a unix socket with
  non-blocking clients, so a stalled reader can't stall the engine.
* **Time / NTP.** The engine asks for NTP time over UDP. A local responder answers from the system
  clock, which systemd-timesyncd keeps correct.

## Findings
* Two-way communication works at 921 600 baud. The engine downloads its configuration, programs
  the RF front end (`GL_RF_4752_BRCM_EXT_LNA`, 26 MHz TCXO), and runs a cold start using its
  built-in almanac (GPS, GLONASS, BeiDou, QZSS, SBAS).
* The first version of the serial layer ran overlapping reads in parallel threads. The engine then
  reported `PacketMgr::Garbage received` and finally "ASIC communication failure". In-order queues
  fixed it.
* The engine doesn't open `NmeaOutName` in this configuration; positions come only through the
  callback.
* **GPS week rollover.** The 2015 engine maps today's GPS week (≥ 2400) into the 1999–2019 era and
  reports dates in 2007. The position is correct; only the date is wrong. `tabpro-s-gnss` puts the
  system's UTC date in the NMEA when the engine's date is more than a day off. `Utc2GpsOff` is set
  to 18 leap seconds (the engine's default is 14).
* Position accuracy checked against a known location: GPS-only and GPS + GLONASS fixes agree within
  about 16 m, with about 6 m scatter. If a map shows you kilometres away, it is showing GeoClue's
  coarse Wi-Fi/IP estimate from before the GPS fix.

## Location report layout (`GpsHalSetLocSensorFuncPtr`, 0x910 bytes, little endian)
| offset | type | field |
|---|---|---|
| 0 | u32 | engine time (ms) |
| 4–18 | u16 × 8 | UTC year, month, day, hour, minute, second, ms, µs |
| 24 / 28 | u32 | GPS week / time of week (ms) |
| 44 | u32 | fix type (3 = 3D) |
| 56 / 64 | f64 | latitude / longitude (degrees) |
| 72 / 80 | f64 | altitude above ellipsoid / above sea level (m) |
| 96–112 | f64 | DOPs (HDOP at 96) |
| 120 | f64 | horizontal accuracy (m) |
| 152 / 184 | f64 | speed (m/s) / heading (degrees) |
| 198 | u16 | satellites used, followed by their ids |
