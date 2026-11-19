# gnss/ – Broadcom BCM4752 GPS/GNSS (work in progress)

**Status:** ✅ works (manual setup): the engine gets a fix (first test indoors near a window: 9 satellites, GPS + GLONASS, about 12 m), and the result reaches GeoClue / GNOME Maps as NMEA. Packaging still to do.

The TabPro S has a Broadcom **BCM4752** GNSS receiver (ACPI `SSE4752`, `\_SB.GPS0`) on the LPSS UART
`ttyS4`: 115 200 baud at power-up, hardware flow control, one GPIO (`RPIN`, "REGPU" = power enable).
The chip has no positioning firmware of its own. All position computation happens on the host in
Broadcom's closed **GLL engine** (`BcmGnss.dll`, v19.13 / GLL 17.19.13), which speaks a binary
protocol to the chip.

## Pieces
| | |
|---|---|
| [`kernel/bcm4752`](kernel/bcm4752/bcm4752.c) | serdev + GNSS-subsystem driver: `/dev/gnssN` raw pass-through, hardware flow control, sysfs `speed` (UART baud, set by the engine) and `regpu` (power GPIO) |
| [`tabpro-s-gnss`](tabpro-s-gnss/) | Rust program that **loads `BcmGnss.dll` natively** (no Wine): a PE loader plus about 200 Windows API functions (threads, events, APC-completion I/O, critical sections, TLS, files, time, serial port on `/dev/gnssN`, the GPIO helper DLL built in; registry, COM, sockets and WinHTTP are stubs), plus a fake TEB in `GS` for the MSVC code |

How the engine is driven (from `BcmGnssLocationSensor.dll`): `glgps_main(0, NULL, "gpsconfig.xml", 0)`
in a thread, then `GpsHalPlatfPostIrmCommand("$pglirm,req_pos,win_xp_immediate_periodic_request,acc,50")`.
Diagnostics come through `GpsHalSetDriverTraceFuncPtr`, positions through `GpsHalSetLocSensorFuncPtr`
(a 0x910-byte report).

## Trying it
`BcmGnss.dll` and `BcmGnssConfig.xml` come from your own Windows driver
(`C:\Windows\System32\DriverStore\FileRepository\bcmgnsslocationsensor.inf_*`); they are not
redistributed here.
```
make -C /lib/modules/$(uname -r)/build M=$PWD/kernel/bcm4752 modules   # + sign for Secure Boot
sudo insmod kernel/bcm4752/bcm4752.ko
cargo build --release --manifest-path tabpro-s-gnss/Cargo.toml
sudo install -d /var/lib/tabpro-s-gnss && sudo cp BcmGnss.dll /var/lib/tabpro-s-gnss/
sudo tabpro-s-gnss/target/release/tabpro-s-gnss --make-config BcmGnssConfig.xml
sudo tabpro-s-gnss/target/release/tabpro-s-gnss -vv
```
`-vvv` also logs every byte on the UART.

## Findings so far
* Two-way communication works at 921 600 baud. The engine downloads its configuration, programs
  the RF front end (`GL_RF_4752_BRCM_EXT_LNA`, 26 MHz TCXO), and runs a cold start using its
  built-in almanac (GPS, GLONASS, BeiDou, QZSS, SBAS).
* The first version of the serial layer ran overlapping asynchronous reads in parallel threads. The
  engine reported `PacketMgr::Garbage received` and finally "ASIC communication failure". Requests
  now run strictly in order, like a Windows serial driver.
* The engine does not open `NmeaOutName` in this configuration. Positions arrive through the
  location-sensor callback. `tabpro-s-gnss` turns each report into `$GPGGA` + `$GPRMC` on
  `/run/tabpro-s-gnss/nmea.sock`.

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

## GeoClue
```
printf '[network-nmea]\nenable=true\nnmea-socket=/run/tabpro-s-gnss/nmea.sock\n' | \
  sudo tee /etc/geoclue/conf.d/90-tabpro-s-gnss.conf
# SELinux: geoclue_t may not connect to an unconfined service's socket by default
checkmodule -M -m -o t.mod selinux/tabpro_s_gnss.te && semodule_package -o t.pp -m t.mod && sudo semodule -i t.pp
sudo systemctl restart geoclue
```
