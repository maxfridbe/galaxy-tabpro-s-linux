//! NMEA output: lines written by the engine to NmeaOutName are served on a unix socket (GeoClue,
//! gpsd-style readers) and summarised on stderr.

use crate::log;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;

static CLIENTS: Mutex<Vec<UnixStream>> = Mutex::new(Vec::new());
static PARTIAL: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static LAST_LOG: Mutex<Option<std::time::Instant>> = Mutex::new(None);

pub fn serve(path: &str) {
    let _ = std::fs::remove_file(path);
    if let Some(d) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let l = match UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) => {
            log!(0, "nmea: cannot bind {path}: {e}");
            return;
        }
    };
    let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o666));
    log!(0, "NMEA on unix socket {path}");
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            CLIENTS.lock().unwrap().push(c);
        }
    });
}

pub fn output(data: &[u8]) {
    let mut p = PARTIAL.lock().unwrap();
    p.extend_from_slice(data);
    while let Some(i) = p.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = p.drain(..=i).collect();
        let s = String::from_utf8_lossy(&line).trim().to_string();
        if s.is_empty() {
            continue;
        }
        if s.contains("GGA") || s.contains("RMC") {
            let mut ll = LAST_LOG.lock().unwrap();
            if ll.is_none_or(|t| t.elapsed().as_secs() >= 10) {
                log!(0, "nmea: {s}");
                *ll = Some(std::time::Instant::now());
            }
        }
        log!(2, "nmea: {s}");
        let out = format!("{s}\r\n");
        CLIENTS.lock().unwrap().retain_mut(|c| c.write_all(out.as_bytes()).is_ok());
    }
}

fn checksum(body: &str) -> String {
    let c = body.bytes().fold(0u8, |a, b| a ^ b);
    format!("${body}*{c:02X}")
}

fn ddmm(v: f64, pos: char, neg: char, deg_digits: usize) -> String {
    let h = if v < 0.0 { neg } else { pos };
    let a = v.abs();
    let d = a.trunc();
    let m = (a - d) * 60.0;
    format!("{:0w$}{:07.4},{h}", d as u32, m, w = deg_digits)
}

/// Turn the engine's location report (GpsHalSetLocSensorFuncPtr; layout reverse-engineered,
/// see gnss/README.md) into $GPGGA + $GPRMC and send them to the socket clients.
pub fn location_report(buf: usize, size: usize) {
    if size < 0x100 {
        return;
    }
    let b = unsafe { std::slice::from_raw_parts(buf as *const u8, 0x100) };
    let u16_ = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_ = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let f64_ = |o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let (year, month, day, hour, min, sec, ms) =
        (u16_(4), u16_(6), u16_(8), u16_(10), u16_(12), u16_(14), u16_(16));
    let fix_type = u32_(44);
    let (lat, lon, alt_ell, alt_msl) = (f64_(56), f64_(64), f64_(72), f64_(80));
    let hdop = f64_(96);
    let acc = f64_(120);
    let speed = f64_(152);
    let course = f64_(184);
    let nsat = u16_(198);
    let valid = fix_type >= 2 && (lat != 0.0 || lon != 0.0) && acc < 1000.0 && year >= 2020;
    log!(2, "report: fix_type={fix_type} acc={acc:.1} m hdop={hdop:.1} sats={nsat} valid={valid}");
    if year < 2020 {
        return; // no time yet
    }
    let t = format!("{hour:02}{min:02}{sec:02}.{:02}", ms / 10);
    let (la, lo) = if valid { (ddmm(lat, 'N', 'S', 2), ddmm(lon, 'E', 'W', 3)) } else { (",".into(), ",".into()) };
    let gga = if valid {
        format!("GPGGA,{t},{la},{lo},1,{nsat:02},{hdop:.1},{alt_msl:.1},M,{:.1},M,,", alt_ell - alt_msl)
    } else {
        format!("GPGGA,{t},{la},{lo},0,00,,,M,,M,,")
    };
    let rmc = format!(
        "GPRMC,{t},{},{la},{lo},{:.1},{:.1},{day:02}{month:02}{:02},,,{}",
        if valid { 'A' } else { 'V' },
        speed * 1.943_844,
        course,
        year % 100,
        if valid { 'A' } else { 'N' }
    );
    output(format!("{}\r\n{}\r\n", checksum(&gga), checksum(&rmc)).as_bytes());
}
