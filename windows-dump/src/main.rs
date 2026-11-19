//! w700dump — snapshot the Windows device tree + firmware tables of a machine
//! onto the USB stick it is run from, so Linux drivers (sound/touch) can be
//! worked out later.
//!
//! Output: <dir of exe>\w700-backup\<COMPUTERNAME>-<YYYYmmdd-HHMMSS>\
//!   devices.json        every device SetupAPI knows (present + ghost), all properties
//!   device-tree.txt     live PnP tree (CM_* walk) with problem codes
//!   summary.txt         audio / touch / HID / sensor / problem devices, human readable
//!   acpi\*.bin          raw ACPI tables (DSDT, SSDTs, ...) -> `iasl -d` on Linux
//!   smbios.bin          raw SMBIOS (RSMB)
//!   reg\*.reg           registry exports (Enum, Class, Services, HARDWARE, MMDevices, ...)
//!   cmd\*.txt           output of pnputil/driverquery/systeminfo/msinfo32/dxdiag/...
//!   drivers\oemN\       third-party drivers for audio/HID/touch/sensor/problem devices
//!   run.log             what happened

use serde_json::{Map, Value, json};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Properties::{DEVPROPKEY, DEVPROPTYPE};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, SYSTEMTIME};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::SystemInformation::{
    EnumSystemFirmwareTables, GetLocalTime, GetSystemFirmwareTable,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::core::GUID;

const DRIVER_EXPORT_CAP: u64 = 400 * 1024 * 1024;

// ---------------------------------------------------------------- logging

struct Log {
    file: Option<fs::File>,
}
impl Log {
    fn line(&mut self, s: &str) {
        println!("{s}");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{s}");
        }
    }
}
macro_rules! log { ($l:expr, $($t:tt)*) => { $l.line(&format!($($t)*)) } }

// ---------------------------------------------------------------- helpers

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
fn guid_str(g: &GUID) -> String {
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7]
    )
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn is_elevated() -> bool {
    unsafe {
        let mut tok: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
            return false;
        }
        let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            tok,
            TokenElevation,
            &mut el as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        CloseHandle(tok);
        ok != 0 && el.TokenIsElevated != 0
    }
}

/// Re-launch ourselves elevated (UAC prompt). Returns true if the elevated copy ran.
fn relaunch_elevated() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let file = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let params = wide("--elevated");
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
    sei.nShow = SW_SHOWNORMAL;
    unsafe {
        if ShellExecuteExW(&mut sei) == 0 {
            return false;
        }
        if !sei.hProcess.is_null() {
            WaitForSingleObject(sei.hProcess, INFINITE);
            CloseHandle(sei.hProcess);
        }
    }
    true
}

fn pause() {
    print!("\nPress Enter to close...");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
}

/// Run a command with a timeout, saving stdout+stderr to `out`. Returns exit success.
fn run_cmd(log: &mut Log, out: &Path, timeout_s: u64, prog: &str, args: &[&str]) -> bool {
    let started = Instant::now();
    let child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            log!(log, "  [skip] {prog} {}: {e}", args.join(" "));
            return false;
        }
    };
    // Drain pipes on threads so big outputs don't deadlock.
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = io::Read::read_to_end(&mut so, &mut v);
        v
    });
    let t2 = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = io::Read::read_to_end(&mut se, &mut v);
        v
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() > Duration::from_secs(timeout_s) => {
                let _ = child.kill();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break None,
        }
    };
    let mut data = t1.join().unwrap_or_default();
    let err = t2.join().unwrap_or_default();
    if !err.is_empty() {
        data.extend_from_slice(b"\r\n----- stderr -----\r\n");
        data.extend_from_slice(&err);
    }
    if !data.is_empty() {
        let _ = fs::write(out, &data);
    }
    let ok = status.map(|s| s.success()).unwrap_or(false);
    log!(
        log,
        "  [{}] {prog} {} ({:.1}s){}",
        if ok { " ok " } else { "FAIL" },
        args.join(" "),
        started.elapsed().as_secs_f32(),
        if status.is_none() { " TIMEOUT" } else { "" }
    );
    ok
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => total += dir_size(&e.path()),
                Ok(m) => total += m.len(),
                _ => {}
            }
        }
    }
    total
}

// ---------------------------------------------------------------- device properties

/// Friendly names for the DEVPKEYs we care about. Unknown keys are kept under their raw id.
fn key_name(k: &DEVPROPKEY) -> Option<&'static str> {
    let g = guid_str(&k.fmtid);
    Some(match (g.as_str(), k.pid) {
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 2) => "DeviceDesc",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 3) => "HardwareIds",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 4) => "CompatibleIds",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 6) => "Service",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 9) => "Class",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 10) => "ClassGuid",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 11) => "Driver",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 12) => "ConfigFlags",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 13) => "Manufacturer",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 14) => "FriendlyName",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 15) => "LocationInfo",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 16) => "PDOName",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 17) => "Capabilities",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 18) => "UINumber",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 19) => "UpperFilters",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 20) => "LowerFilters",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 21) => "BusTypeGuid",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 22) => "LegacyBusType",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 23) => "BusNumber",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 27) => "DevType",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 29) => "Characteristics",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 32) => "PowerData",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 33) => "RemovalPolicy",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 36) => "InstallState",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 24) => "EnumeratorName",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 30) => "Address",
        ("{a45c254e-df1c-4efd-8020-67d146a850e0}", 37) => "LocationPaths",
        ("{4340a6c5-93fa-4706-972c-7b648008a5a7}", 2) => "DevNodeStatus",
        ("{4340a6c5-93fa-4706-972c-7b648008a5a7}", 3) => "ProblemCode",
        ("{4340a6c5-93fa-4706-972c-7b648008a5a7}", 8) => "Parent",
        ("{4340a6c5-93fa-4706-972c-7b648008a5a7}", 9) => "Children",
        ("{4340a6c5-93fa-4706-972c-7b648008a5a7}", 10) => "Siblings",
        ("{540b947e-8b40-45bc-a8a2-6a0b894cbda2}", 4) => "BusReportedDeviceDesc",
        ("{540b947e-8b40-45bc-a8a2-6a0b894cbda2}", 10) => "BiosDeviceName",
        ("{78c34fc8-104a-4aca-9ea4-524d52996e57}", 256) => "InstanceId",
        ("{83da6326-97a6-4088-9453-a1923f573b29}", 3) => "IsPresent",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 2) => "DriverDate",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 3) => "DriverVersion",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 4) => "DriverDesc",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 5) => "DriverInfPath",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 6) => "DriverInfSection",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 8) => "MatchingDeviceId",
        ("{a8b865dd-2e3d-4094-ad97-e593a70c75d6}", 9) => "DriverProvider",
        _ => return None,
    })
}

const T_EMPTY: u32 = 0x0;
const T_NULL: u32 = 0x1;
const T_SBYTE: u32 = 0x2;
const T_BYTE: u32 = 0x3;
const T_INT16: u32 = 0x4;
const T_UINT16: u32 = 0x5;
const T_INT32: u32 = 0x6;
const T_UINT32: u32 = 0x7;
const T_INT64: u32 = 0x8;
const T_UINT64: u32 = 0x9;
const T_GUID: u32 = 0xd;
const T_FILETIME: u32 = 0x10;
const T_BOOLEAN: u32 = 0x11;
const T_STRING: u32 = 0x12;
const T_SECDESC_STR: u32 = 0x14;
const T_ERROR: u32 = 0x16;
const T_NTSTATUS: u32 = 0x17;
const T_STRING_INDIRECT: u32 = 0x19;
const MOD_LIST: u32 = 0x2000;

fn rd<const N: usize>(b: &[u8]) -> [u8; N] {
    let mut a = [0u8; N];
    let n = b.len().min(N);
    a[..n].copy_from_slice(&b[..n]);
    a
}

fn filetime_str(ft: u64) -> String {
    // FILETIME: 100ns since 1601-01-01. Convert to unix secs and format as UTC date.
    if ft == 0 {
        return "0".into();
    }
    let secs = (ft / 10_000_000) as i64 - 11_644_473_600;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn decode_prop(ty: DEVPROPTYPE, b: &[u8]) -> Value {
    let base = ty & 0x0fff;
    let wstr = |b: &[u8]| -> Vec<u16> {
        b.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    };
    if ty & MOD_LIST != 0 && matches!(base, T_STRING | T_STRING_INDIRECT | T_SECDESC_STR) {
        let w = wstr(b);
        let v: Vec<Value> = w
            .split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(String::from_utf16_lossy(s)))
            .collect();
        return Value::Array(v);
    }
    if ty & 0xf000 != 0 {
        // array / list of fixed-size types: just hex it
        return json!({ "type": format!("0x{ty:x}"), "hex": hex(b) });
    }
    match base {
        T_EMPTY | T_NULL => Value::Null,
        T_STRING | T_STRING_INDIRECT | T_SECDESC_STR => Value::String(from_wide(&wstr(b))),
        T_BOOLEAN => Value::Bool(b.first().copied().unwrap_or(0) != 0),
        T_SBYTE => json!(b.first().map(|&x| x as i8).unwrap_or(0)),
        T_BYTE => json!(b.first().copied().unwrap_or(0)),
        T_INT16 => json!(i16::from_le_bytes(rd(b))),
        T_UINT16 => json!(u16::from_le_bytes(rd(b))),
        T_INT32 | T_ERROR | T_NTSTATUS => json!(i32::from_le_bytes(rd(b))),
        T_UINT32 => json!(u32::from_le_bytes(rd(b))),
        T_INT64 => json!(i64::from_le_bytes(rd(b))),
        T_UINT64 => json!(u64::from_le_bytes(rd(b))),
        T_FILETIME => Value::String(filetime_str(u64::from_le_bytes(rd(b)))),
        T_GUID if b.len() >= 16 => {
            let g = GUID {
                data1: u32::from_le_bytes(rd(&b[0..4])),
                data2: u16::from_le_bytes(rd(&b[4..6])),
                data3: u16::from_le_bytes(rd(&b[6..8])),
                data4: rd(&b[8..16]),
            };
            Value::String(guid_str(&g))
        }
        _ => json!({ "type": format!("0x{ty:x}"), "hex": hex(b) }),
    }
}

fn collect_devices(log: &mut Log) -> Vec<Value> {
    let mut out = Vec::new();
    unsafe {
        // All classes, present AND non-present ("ghost") devices.
        let h = SetupDiGetClassDevsW(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_ALLCLASSES,
        );
        if h == -1 {
            log!(log, "  SetupDiGetClassDevsW failed");
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut dd: SP_DEVINFO_DATA = std::mem::zeroed();
            dd.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(h, i, &mut dd) == 0 {
                break;
            }
            i += 1;
            let mut obj = Map::new();

            let mut idbuf = vec![0u16; 1024];
            let mut req = 0u32;
            if SetupDiGetDeviceInstanceIdW(h, &dd, idbuf.as_mut_ptr(), idbuf.len() as u32, &mut req)
                != 0
            {
                obj.insert("_id".into(), Value::String(from_wide(&idbuf)));
            }
            obj.insert("_class_guid".into(), Value::String(guid_str(&dd.ClassGuid)));

            let mut status = 0u32;
            let mut problem = 0u32;
            let present =
                CM_Get_DevNode_Status(&mut status, &mut problem, dd.DevInst, 0) == CR_SUCCESS;
            obj.insert("_present".into(), Value::Bool(present));
            if present {
                obj.insert("_status".into(), json!(format!("0x{status:08x}")));
                obj.insert("_problem".into(), json!(problem));
            }

            // Enumerate every property key this device has.
            let mut nkeys = 0u32;
            SetupDiGetDevicePropertyKeys(h, &dd, std::ptr::null_mut(), 0, &mut nkeys, 0);
            let mut keys: Vec<DEVPROPKEY> = vec![std::mem::zeroed(); nkeys as usize];
            let mut props = Map::new();
            if nkeys > 0
                && SetupDiGetDevicePropertyKeys(h, &dd, keys.as_mut_ptr(), nkeys, &mut nkeys, 0)
                    != 0
            {
                for k in keys.iter().take(nkeys as usize) {
                    let mut ty: DEVPROPTYPE = 0;
                    let mut need = 0u32;
                    SetupDiGetDevicePropertyW(
                        h,
                        &dd,
                        k,
                        &mut ty,
                        std::ptr::null_mut(),
                        0,
                        &mut need,
                        0,
                    );
                    let mut buf = vec![0u8; need.max(1) as usize];
                    if SetupDiGetDevicePropertyW(
                        h,
                        &dd,
                        k,
                        &mut ty,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        &mut need,
                        0,
                    ) == 0
                    {
                        continue;
                    }
                    buf.truncate(need as usize);
                    let v = decode_prop(ty, &buf);
                    let name = match key_name(k) {
                        Some(n) => n.to_string(),
                        None => format!("{} {}", guid_str(&k.fmtid), k.pid),
                    };
                    props.insert(name, v);
                }
            }
            obj.insert("props".into(), Value::Object(props));
            out.push(Value::Object(obj));
        }
        SetupDiDestroyDeviceInfoList(h);
    }
    log!(log, "  {} devices (present + ghost)", out.len());
    out
}

// ---------------------------------------------------------------- live tree

fn dev_id(dn: u32) -> String {
    let mut buf = vec![0u16; 1024];
    unsafe {
        if CM_Get_Device_IDW(dn, buf.as_mut_ptr(), buf.len() as u32, 0) == CR_SUCCESS {
            return from_wide(&buf);
        }
    }
    "?".into()
}

fn walk_tree(
    dn: u32,
    depth: usize,
    names: &std::collections::HashMap<String, String>,
    out: &mut String,
) {
    let id = dev_id(dn);
    let mut status = 0u32;
    let mut problem = 0u32;
    unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, dn, 0) };
    let name = names.get(&id.to_uppercase()).cloned().unwrap_or_default();
    let prob = if problem != 0 {
        format!("  !! PROBLEM {problem}")
    } else {
        String::new()
    };
    out.push_str(&format!(
        "{}{}  [{}]{}\r\n",
        "  ".repeat(depth),
        id,
        name,
        prob
    ));
    let mut child = 0u32;
    unsafe {
        if CM_Get_Child(&mut child, dn, 0) != CR_SUCCESS {
            return;
        }
    }
    loop {
        walk_tree(child, depth + 1, names, out);
        let mut sib = 0u32;
        unsafe {
            if CM_Get_Sibling(&mut sib, child, 0) != CR_SUCCESS {
                break;
            }
        }
        child = sib;
    }
}

// ---------------------------------------------------------------- firmware tables

fn dump_firmware(log: &mut Log, out: &Path) {
    let acpi_dir = out.join("acpi");
    let _ = fs::create_dir_all(&acpi_dir);
    let acpi = u32::from_be_bytes(*b"ACPI");
    unsafe {
        let n = EnumSystemFirmwareTables(acpi, std::ptr::null_mut(), 0);
        let mut ids = vec![0u8; n as usize];
        let n = EnumSystemFirmwareTables(acpi, ids.as_mut_ptr(), n);
        ids.truncate(n as usize);
        if !ids.chunks_exact(4).any(|c| c == b"DSDT") {
            ids.extend_from_slice(b"DSDT"); // not enumerated, but fetchable by ID
        }
        let mut seen = std::collections::HashMap::<u32, u32>::new();
        let mut list = Vec::new();
        for c in ids.chunks_exact(4) {
            let id = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let sig = String::from_utf8_lossy(c).to_string();
            list.push(sig.clone());
            let cnt = seen.entry(id).or_insert(0);
            *cnt += 1;
            if *cnt > 1 {
                continue; // GetSystemFirmwareTable only returns the first table of a given signature
            }
            let sz = GetSystemFirmwareTable(acpi, id, std::ptr::null_mut(), 0);
            if sz == 0 {
                continue;
            }
            let mut buf = vec![0u8; sz as usize];
            let got = GetSystemFirmwareTable(acpi, id, buf.as_mut_ptr(), sz);
            if got == sz {
                let _ = fs::write(acpi_dir.join(format!("{}.bin", sanitize(&sig))), &buf);
            }
        }
        let dups: Vec<String> = seen
            .iter()
            .filter(|(_, c)| **c > 1)
            .map(|(id, c)| format!("{} x{c}", String::from_utf8_lossy(&id.to_le_bytes())))
            .collect();
        log!(log, "  ACPI tables: {}", list.join(" "));
        if !dups.is_empty() {
            log!(
                log,
                "  note: only first copy saved for {} (Windows API limit; full set via Linux acpidump later)",
                dups.join(", ")
            );
        }
        let _ = fs::write(acpi_dir.join("tables.txt"), list.join("\r\n"));

        let rsmb = u32::from_be_bytes(*b"RSMB");
        let sz = GetSystemFirmwareTable(rsmb, 0, std::ptr::null_mut(), 0);
        if sz > 0 {
            let mut buf = vec![0u8; sz as usize];
            if GetSystemFirmwareTable(rsmb, 0, buf.as_mut_ptr(), sz) == sz {
                let _ = fs::write(out.join("smbios.bin"), &buf);
                log!(log, "  SMBIOS: {sz} bytes");
            }
        }
    }
}

// ---------------------------------------------------------------- summary

fn prop_str(d: &Value, k: &str) -> String {
    match &d["props"][k] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

fn interesting(d: &Value) -> Option<&'static str> {
    let class = prop_str(d, "Class").to_lowercase();
    let id = d["_id"].as_str().unwrap_or("").to_uppercase();
    let hw = prop_str(d, "HardwareIds").to_uppercase();
    let desc = format!(
        "{} {}",
        prop_str(d, "DeviceDesc"),
        prop_str(d, "FriendlyName")
    )
    .to_lowercase();
    if d["_present"] == true && d["_problem"].as_u64().unwrap_or(0) != 0 {
        return Some("PROBLEM");
    }
    if d["_present"] == true && class.is_empty() {
        return Some("NO DRIVER");
    }
    if matches!(
        class.as_str(),
        "media" | "audioendpoint" | "audioprocessingobject"
    ) || id.starts_with("HDAUDIO")
        || hw.contains("INTELAUDIO")
        || desc.contains("audio")
        || desc.contains("sound")
        || desc.contains("speaker")
        || desc.contains("microphone")
    {
        return Some("AUDIO");
    }
    if desc.contains("touch")
        || desc.contains("digitizer")
        || desc.contains("pen")
        || desc.contains("wacom")
        || hw.contains("WACF")
        || hw.contains("WCOM")
        || hw.contains("ATML")
        || hw.contains("EGALAX")
        || hw.contains("EETI")
        || hw.contains("ELAN")
        || hw.contains("NTRG")
        || hw.contains("SYNP")
        || hw.contains("UP:000D")
    {
        return Some("TOUCH/PEN");
    }
    if matches!(class.as_str(), "hidclass" | "mouse" | "keyboard")
        || id.starts_with("HID\\")
        || hw.contains("PNP0C50")
    {
        return Some("HID/INPUT");
    }
    if class == "sensor"
        || desc.contains("sensor")
        || desc.contains("accelerometer")
        || desc.contains("gyro")
        || desc.contains("light")
    {
        return Some("SENSOR");
    }
    if id.starts_with("ACPI\\") && d["_present"] == true {
        return Some("ACPI");
    }
    None
}

fn write_summary(out: &Path, devs: &[Value]) {
    let mut s = String::new();
    s.push_str("w700dump summary — sections: PROBLEM, NO DRIVER, AUDIO, TOUCH/PEN, HID/INPUT, SENSOR, ACPI (present only)\r\n");
    s.push_str("Full detail for every device is in devices.json.\r\n");
    for sect in [
        "PROBLEM",
        "NO DRIVER",
        "AUDIO",
        "TOUCH/PEN",
        "HID/INPUT",
        "SENSOR",
        "ACPI",
    ] {
        s.push_str(&format!(
            "\r\n==================== {sect} ====================\r\n"
        ));
        for d in devs
            .iter()
            .filter(|d| d["_present"] == true && interesting(d) == Some(sect))
        {
            s.push_str(&format!("\r\n* {}\r\n", d["_id"].as_str().unwrap_or("?")));
            for k in [
                "DeviceDesc",
                "FriendlyName",
                "BusReportedDeviceDesc",
                "Class",
                "Manufacturer",
                "HardwareIds",
                "CompatibleIds",
                "Service",
                "LowerFilters",
                "UpperFilters",
                "DriverProvider",
                "DriverVersion",
                "DriverDate",
                "DriverInfPath",
                "DriverInfSection",
                "MatchingDeviceId",
                "LocationInfo",
                "LocationPaths",
                "BiosDeviceName",
                "PDOName",
                "Parent",
            ] {
                let v = prop_str(d, k);
                if !v.is_empty() {
                    s.push_str(&format!("    {k:22} {v}\r\n"));
                }
            }
            if let Some(p) = d["_problem"].as_u64().filter(|&p| p != 0) {
                s.push_str(&format!("    {:22} {p}\r\n", "ProblemCode"));
            }
        }
    }
    let _ = fs::write(out.join("summary.txt"), s);
}

// ---------------------------------------------------------------- driver export

fn export_drivers(log: &mut Log, out: &Path, devs: &[Value]) {
    let ddir = out.join("drivers");
    let _ = fs::create_dir_all(&ddir);
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let mut infs: Vec<String> = devs
        .iter()
        .filter(|d| {
            d["_present"] == true
                && matches!(
                    interesting(d),
                    Some("AUDIO" | "TOUCH/PEN" | "HID/INPUT" | "SENSOR" | "PROBLEM" | "ACPI")
                )
        })
        .map(|d| prop_str(d, "DriverInfPath").to_lowercase())
        .filter(|p| p.starts_with("oem") && p.ends_with(".inf"))
        .collect();
    infs.sort();
    infs.dedup();
    log!(
        log,
        "  third-party INFs to export: {}",
        if infs.is_empty() {
            "(none)".into()
        } else {
            infs.join(" ")
        }
    );
    for inf in infs {
        if dir_size(&ddir) > DRIVER_EXPORT_CAP {
            log!(log, "  driver export size cap reached, stopping");
            break;
        }
        let dst = ddir.join(inf.trim_end_matches(".inf"));
        let _ = fs::create_dir_all(&dst);
        let dst_s = dst.to_string_lossy().to_string();
        let ok = run_cmd(
            log,
            &dst.join("pnputil.txt"),
            120,
            "pnputil",
            &["/export-driver", &inf, &dst_s],
        );
        if !ok {
            // Old pnputil (Win 8.x): at least keep the INF, it holds codec pin configs, HW IDs, etc.
            let src = Path::new(&windir).join("INF").join(&inf);
            match fs::copy(&src, dst.join(&inf)) {
                Ok(_) => log!(log, "  copied {} instead", src.display()),
                Err(e) => log!(log, "  could not copy {}: {e}", src.display()),
            }
        }
    }
}

// ---------------------------------------------------------------- main

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let already_elevated_attempt = args.iter().any(|a| a == "--elevated");
    let no_pause = args.iter().any(|a| a == "--no-pause");

    if !is_elevated() && !already_elevated_attempt {
        println!("w700dump: not running as Administrator — asking for elevation (UAC)...");
        if relaunch_elevated() {
            return; // the elevated copy did the work (and paused in its own window)
        }
        println!(
            "Elevation declined/failed; continuing with limited rights (some data will be missing)."
        );
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    let out = exe_dir.join("w700-backup").join(format!(
        "{}-{:04}{:02}{:02}-{:02}{:02}{:02}",
        sanitize(&host),
        st.wYear,
        st.wMonth,
        st.wDay,
        st.wHour,
        st.wMinute,
        st.wSecond
    ));
    if let Err(e) = fs::create_dir_all(&out) {
        eprintln!("cannot create {}: {e}", out.display());
        if !no_pause {
            pause();
        }
        return;
    }
    let mut log = Log {
        file: fs::File::create(out.join("run.log")).ok(),
    };
    log!(
        log,
        "w700dump {} -> {}",
        env!("CARGO_PKG_VERSION"),
        out.display()
    );
    log!(log, "elevated: {}", is_elevated());

    log!(log, "\n[1/6] firmware tables");
    dump_firmware(&mut log, &out);

    log!(log, "\n[2/6] SetupAPI device enumeration");
    let devs = collect_devices(&mut log);
    let _ = fs::write(
        out.join("devices.json"),
        serde_json::to_string_pretty(&Value::Array(devs.clone())).unwrap_or_default(),
    );

    log!(log, "\n[3/6] live PnP tree");
    let names: std::collections::HashMap<String, String> = devs
        .iter()
        .filter_map(|d| {
            let id = d["_id"].as_str()?.to_uppercase();
            let n = prop_str(d, "FriendlyName");
            Some((
                id,
                if n.is_empty() {
                    prop_str(d, "DeviceDesc")
                } else {
                    n
                },
            ))
        })
        .collect();
    let mut root = 0u32;
    let mut tree = String::new();
    unsafe {
        if CM_Locate_DevNodeW(&mut root, std::ptr::null(), CM_LOCATE_DEVNODE_NORMAL) == CR_SUCCESS {
            walk_tree(root, 0, &names, &mut tree);
        }
    }
    let _ = fs::write(out.join("device-tree.txt"), &tree);
    log!(log, "  {} nodes in tree", tree.lines().count());
    write_summary(&out, &devs);

    log!(log, "\n[4/6] registry exports");
    let reg = out.join("reg");
    let _ = fs::create_dir_all(&reg);
    for (name, key) in [
        ("Enum", r"HKLM\SYSTEM\CurrentControlSet\Enum"),
        ("Class", r"HKLM\SYSTEM\CurrentControlSet\Control\Class"),
        (
            "DeviceClasses",
            r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceClasses",
        ),
        ("Services", r"HKLM\SYSTEM\CurrentControlSet\Services"),
        ("HARDWARE", r"HKLM\HARDWARE"),
        (
            "MMDevices",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices",
        ),
        ("Realtek", r"HKLM\SOFTWARE\Realtek"),
        ("Wacom", r"HKLM\SOFTWARE\Wacom"),
        ("Samsung", r"HKLM\SOFTWARE\Samsung"),
        (
            "TouchPrediction",
            r"HKLM\SOFTWARE\Microsoft\TouchPrediction",
        ),
        ("Wisp", r"HKLM\SOFTWARE\Microsoft\Wisp"),
    ] {
        let f = reg.join(format!("{name}.reg"));
        run_cmd(
            &mut log,
            &reg.join(format!("{name}.log")),
            180,
            "reg",
            &["export", key, &f.to_string_lossy(), "/y"],
        );
    }

    log!(log, "\n[5/6] system tools");
    let cmd = out.join("cmd");
    let _ = fs::create_dir_all(&cmd);
    let c = |n: &str| cmd.join(n);
    run_cmd(&mut log, &c("ver.txt"), 20, "cmd", &["/c", "ver"]);
    run_cmd(&mut log, &c("systeminfo.txt"), 120, "systeminfo", &[]);
    if !run_cmd(
        &mut log,
        &c("pnputil-devices.txt"),
        120,
        "pnputil",
        &["/enum-devices", "/drivers", "/ids", "/relations"],
    ) && !run_cmd(
        &mut log,
        &c("pnputil-devices.txt"),
        120,
        "pnputil",
        &["/enum-devices"],
    ) {
        log!(
            log,
            "  (pnputil /enum-devices unsupported on this Windows — devices.json covers it)"
        );
    }
    if !run_cmd(
        &mut log,
        &c("pnputil-drivers.txt"),
        120,
        "pnputil",
        &["/enum-drivers"],
    ) {
        run_cmd(&mut log, &c("pnputil-drivers.txt"), 120, "pnputil", &["-e"]);
    }
    run_cmd(
        &mut log,
        &c("driverquery.csv"),
        120,
        "driverquery",
        &["/v", "/fo", "csv"],
    );
    run_cmd(
        &mut log,
        &c("driverquery-signed.csv"),
        120,
        "driverquery",
        &["/si", "/fo", "csv"],
    );
    run_cmd(
        &mut log,
        &c("powercfg-sleepstates.txt"),
        60,
        "powercfg",
        &["/a"],
    );
    run_cmd(
        &mut log,
        &c("bcdedit.txt"),
        60,
        "bcdedit",
        &["/enum", "all"],
    );
    let ps = |cmdline: &str| -> Vec<String> {
        vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-Command".into(),
            cmdline.into(),
        ]
    };
    for (file, script) in [
        (
            "signed-drivers.txt",
            "Get-CimInstance Win32_PnPSignedDriver | Format-List * | Out-String -Width 4096",
        ),
        (
            "pnp-entities.txt",
            "Get-CimInstance Win32_PnPEntity | Format-List * | Out-String -Width 4096",
        ),
        (
            "sound-devices.txt",
            "Get-CimInstance Win32_SoundDevice | Format-List * | Out-String -Width 4096",
        ),
        (
            "disks.txt",
            "Get-Disk | Format-List *; Get-Partition | Format-List *; Get-Volume | Format-List * | Out-String -Width 4096",
        ),
        (
            "firmware.txt",
            "'firmware_type=' + $env:firmware_type; try { 'SecureBoot=' + (Confirm-SecureBootUEFI) } catch { 'SecureBoot=unknown: ' + $_ }; Get-CimInstance Win32_BIOS | Format-List *; Get-CimInstance Win32_ComputerSystem | Format-List *; Get-CimInstance Win32_BaseBoard | Format-List *; Get-CimInstance Win32_Processor | Format-List *",
        ),
    ] {
        let a = ps(script);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        run_cmd(&mut log, &c(file), 300, "powershell", &a);
    }
    let dx = c("dxdiag.txt").to_string_lossy().to_string();
    run_cmd(
        &mut log,
        &c("dxdiag.log"),
        180,
        "dxdiag",
        &["/whql:off", "/t", &dx],
    );
    let mi = c("msinfo32.txt").to_string_lossy().to_string();
    run_cmd(
        &mut log,
        &c("msinfo32.log"),
        600,
        "msinfo32",
        &["/report", &mi],
    );
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for f in ["setupapi.dev.log", "setupapi.setup.log"] {
        let _ = fs::copy(Path::new(&windir).join("INF").join(f), cmd.join(f));
    }

    log!(
        log,
        "\n[6/6] third-party drivers for audio/touch/HID/sensor/ACPI/problem devices"
    );
    export_drivers(&mut log, &out, &devs);

    log!(
        log,
        "\nDONE. Total size: {:.1} MB",
        dir_size(&out) as f64 / 1048576.0
    );
    log!(log, "Output: {}", out.display());
    log!(
        log,
        "You can unplug the stick now (use 'Safely Remove' first)."
    );
    if let Some(f) = log.file.as_ref() {
        let _ = f.sync_all();
    }
    drop(log);
    if !no_pause {
        pause();
    }
}
