//! KERNEL32 implementations used by BcmGnss.dll (and its statically linked MSVC CRT).
#![allow(non_snake_case, clippy::missing_safety_doc)]

use crate::comm::Comm;
use crate::log;
use crate::rt::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

pub struct Config {
    pub root: String,          // where C:\..., relative paths and the state live
    pub chip_ports: Vec<String>, // names that open the GNSS UART (upper case)
    pub nmea_port: String,
    pub comm: crate::comm::Paths,
    pub image: std::ops::Range<usize>,
}
pub static CONFIG: OnceLock<Config> = OnceLock::new();
fn cfg() -> &'static Config {
    CONFIG.get().unwrap()
}

const TRUE: usize = 1;
const FALSE: usize = 0;
const INVALID: usize = usize::MAX;

/// Windows path -> Linux path.  Z:\x -> /x ; C:\x (any other drive) -> root/x ; relative -> root/x
pub fn to_unix(p: &str) -> String {
    let p = p.trim_start_matches("\\\\?\\");
    let b = p.as_bytes();
    let rest = p.replace('\\', "/");
    if b.len() >= 2 && b[1] == b':' {
        let tail = rest[2..].trim_start_matches('/');
        if b[0].eq_ignore_ascii_case(&b'z') {
            return format!("/{tail}");
        }
        return format!("{}/{}", cfg().root, tail);
    }
    if rest.starts_with('/') {
        return format!("{}{}", cfg().root, rest);
    }
    format!("{}/{}", cfg().root, rest)
}

fn errno_to_win(e: i32) -> u32 {
    match e {
        libc::ENOENT => 2,
        libc::ENOTDIR => 3,
        libc::EACCES | libc::EPERM => 5,
        libc::EEXIST => 80,
        libc::ENOSPC => 112,
        _ => 31,
    }
}
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

// ---- files & devices --------------------------------------------------------------------------
static COMM: OnceLock<Arc<Comm>> = OnceLock::new();

fn create_file(name: String, access: u32, disp: u32) -> usize {
    let dev = name.trim_start_matches("\\\\.\\").to_ascii_uppercase();
    if cfg().chip_ports.iter().any(|p| *p == dev) || (dev.starts_with("COM") && dev[3..].parse::<u32>().is_ok()) {
        let c = match COMM.get() {
            Some(c) => c.clone(),
            None => match Comm::open(&cfg().comm) {
                Ok(c) => COMM.get_or_init(|| c).clone(),
                Err(e) => {
                    log!(0, "CreateFile({name}): cannot open {}: errno {e}", cfg().comm.dev);
                    set_err(errno_to_win(e));
                    return INVALID;
                }
            },
        };
        log!(1, "CreateFile({name}) -> GNSS UART");
        return new_handle(Obj::Comm(c));
    }
    if dev == cfg().nmea_port.to_ascii_uppercase() {
        log!(1, "CreateFile({name}) -> NMEA output");
        return new_handle(Obj::Nmea);
    }
    let path = to_unix(&name);
    let mut fl = libc::O_CLOEXEC;
    fl |= match (access & 0x8000_0000 != 0 || access & 1 != 0, access & 0x4000_0000 != 0 || access & 2 != 0) {
        (_, true) if access & 0x8000_0000 != 0 => libc::O_RDWR,
        (_, true) => libc::O_WRONLY,
        _ => libc::O_RDONLY,
    };
    let existed = std::path::Path::new(&path).exists();
    match disp {
        1 => fl |= libc::O_CREAT | libc::O_EXCL,
        2 => fl |= libc::O_CREAT | libc::O_TRUNC,
        4 => fl |= libc::O_CREAT,
        5 => fl |= libc::O_TRUNC,
        _ => {}
    }
    if fl & libc::O_CREAT != 0 {
        if let Some(d) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(d);
        }
    }
    let c = std::ffi::CString::new(path.clone()).unwrap();
    let fd = unsafe { libc::open(c.as_ptr(), fl, 0o644) };
    if fd < 0 {
        let e = last_errno();
        log!(2, "CreateFile({name} -> {path}) failed errno {e}");
        set_err(errno_to_win(e));
        return INVALID;
    }
    log!(2, "CreateFile({name} -> {path}) = fd {fd}");
    set_err(if existed && (disp == 2 || disp == 4) { 183 } else { 0 });
    new_handle(Obj::File(fd))
}

pub extern "win64" fn CreateFileA(name: usize, access: u32, _share: u32, _sa: usize, disp: u32, _fl: u32, _t: usize) -> usize {
    create_file(unsafe { astr(name) }, access, disp)
}
pub extern "win64" fn CreateFileW(name: usize, access: u32, _share: u32, _sa: usize, disp: u32, _fl: u32, _t: usize) -> usize {
    create_file(unsafe { wstr(name) }, access, disp)
}

pub extern "win64" fn CloseHandle(h: usize) -> usize {
    if close_handle(h) {
        TRUE
    } else {
        set_err(6);
        FALSE
    }
}

/// Completion of an async (overlapped) request: fill OVERLAPPED, signal its event.
fn complete_overlapped(ov: usize, status: u32, bytes: usize) {
    unsafe {
        w64(ov, if status == 0 { 0 } else { 0xC000_0120u64 }); // Internal: STATUS_SUCCESS / CANCELLED
        w64(ov + 8, bytes as u64); // InternalHigh
        let ev = r64(ov + 24) as usize;
        if ev != 0 {
            if let Some(o) = get_handle(ev & !3) {
                if let Obj::Event(e) = &*o {
                    e.set.store(true, Ordering::SeqCst);
                    notify();
                }
            }
        }
    }
}

type Completion = extern "win64" fn(u32, u32, usize);

fn comm_async(c: Arc<Comm>, write: bool, buf: usize, n: usize, ov: usize, routine: usize) {
    let tid = cur_tid();
    let gen = c.generation();
    unsafe { w64(ov, 0x103) }; // STATUS_PENDING
    let c2 = c.clone();
    c.submit(write, Box::new(move || {
        let r = if write {
            c2.write(unsafe { std::slice::from_raw_parts(buf as *const u8, n) })
        } else {
            c2.read_gen(unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, n) }, gen)
        };
        let (err, bytes) = match r {
            Ok(b) => (0u32, b),
            Err(e) => (e, 0),
        };
        if routine != 0 {
            queue_apc(tid, Box::new(move || {
                unsafe {
                    w64(ov, if err == 0 { 0 } else { 0xC000_0120 });
                    w64(ov + 8, bytes as u64);
                }
                let f: Completion = unsafe { std::mem::transmute(routine) };
                f(err, bytes as u32, ov);
            }));
        } else {
            complete_overlapped(ov, err, bytes);
        }
    }));
}

pub extern "win64" fn ReadFile(h: usize, buf: usize, n: u32, read: usize, ov: usize) -> usize {
    let Some(o) = get_handle(h) else { set_err(6); return FALSE };
    match &*o {
        Obj::File(fd) if ov != 0 => file_ov_io(*fd, false, buf, n as usize, ov, 0),
        Obj::File(fd) | Obj::Std(fd) => {
            let r = unsafe { libc::read(*fd, buf as *mut _, n as usize) };
            if r < 0 {
                set_err(errno_to_win(last_errno()));
                return FALSE;
            }
            if read != 0 {
                unsafe { w32(read, r as u32) };
            }
            TRUE
        }
        Obj::Comm(c) => {
            if ov != 0 {
                if read != 0 {
                    unsafe { w32(read, 0) };
                }
                comm_async(c.clone(), false, buf, n as usize, ov, 0);
                set_err(997); // ERROR_IO_PENDING
                return FALSE;
            }
            match c.read(unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, n as usize) }) {
                Ok(b) => {
                    if read != 0 {
                        unsafe { w32(read, b as u32) };
                    }
                    TRUE
                }
                Err(e) => {
                    set_err(e);
                    FALSE
                }
            }
        }
        _ => {
            if read != 0 {
                unsafe { w32(read, 0) };
            }
            TRUE
        }
    }
}

pub extern "win64" fn WriteFile(h: usize, buf: usize, n: u32, written: usize, ov: usize) -> usize {
    let Some(o) = get_handle(h).or_else(|| std_handle(h)) else { set_err(6); return FALSE };
    let data = unsafe { std::slice::from_raw_parts(buf as *const u8, n as usize) };
    let ok = match &*o {
        Obj::File(fd) if ov != 0 => {
            let r = file_ov_io(*fd, true, buf, n as usize, ov, 0);
            if written != 0 {
                unsafe { w32(written, if r != 0 { n } else { 0 }) };
            }
            return r;
        }
        Obj::File(fd) | Obj::Std(fd) => {
            log!(3, "WriteFile(fd {fd}, {n} B)");
            (unsafe { libc::write(*fd, buf as *const _, n as usize) }) >= 0
        }
        Obj::Nmea => {
            crate::nmea::output(data);
            true
        }
        Obj::Comm(c) => {
            if ov != 0 {
                comm_async(c.clone(), true, buf, n as usize, ov, 0);
                set_err(997);
                return FALSE;
            }
            c.write(data).is_ok()
        }
        _ => true,
    };
    if written != 0 {
        unsafe { w32(written, if ok { n } else { 0 }) };
    }
    ok as usize
}


/// Overlapped / Ex I/O on regular files: done synchronously at the OVERLAPPED offset, completion
/// reported the Windows way (event or APC).
fn file_ov_io(fd: i32, write: bool, buf: usize, n: usize, ov: usize, routine: usize) -> usize {
    let off = unsafe { (r32(ov + 16) as u64 | ((r32(ov + 20) as u64) << 32)) as i64 };
    let r = unsafe {
        if write { libc::pwrite(fd, buf as *const _, n, off) } else { libc::pread(fd, buf as *mut _, n, off) }
    };
    let (err, bytes) = if r < 0 { (errno_to_win(last_errno()), 0usize) } else { (0u32, r as usize) };
    if routine != 0 {
        let tid = cur_tid();
        queue_apc(tid, Box::new(move || {
            unsafe {
                w64(ov, if err == 0 { 0 } else { 0xC000_0001 });
                w64(ov + 8, bytes as u64);
            }
            let f: Completion = unsafe { std::mem::transmute(routine) };
            f(err, bytes as u32, ov);
        }));
    } else {
        complete_overlapped(ov, err, bytes);
    }
    if err != 0 {
        set_err(err);
        FALSE
    } else {
        TRUE
    }
}

pub extern "win64" fn ReadFileEx(h: usize, buf: usize, n: u32, ov: usize, routine: usize) -> usize {
    match get_handle(h).as_deref() {
        Some(Obj::Comm(c)) => {
            comm_async(c.clone(), false, buf, n as usize, ov, routine);
            set_err(0);
            TRUE
        }
        Some(Obj::File(fd)) => file_ov_io(*fd, false, buf, n as usize, ov, routine),
        _ => {
            log!(1, "ReadFileEx on unsupported handle {h:#x}");
            set_err(6);
            FALSE
        }
    }
}
pub extern "win64" fn WriteFileEx(h: usize, buf: usize, n: u32, ov: usize, routine: usize) -> usize {
    match get_handle(h).as_deref() {
        Some(Obj::Comm(c)) => {
            comm_async(c.clone(), true, buf, n as usize, ov, routine);
            set_err(0);
            TRUE
        }
        Some(Obj::Nmea) => {
            crate::nmea::output(unsafe { std::slice::from_raw_parts(buf as *const u8, n as usize) });
            let tid = cur_tid();
            queue_apc(tid, Box::new(move || {
                let f: Completion = unsafe { std::mem::transmute(routine) };
                f(0, n, ov);
            }));
            TRUE
        }
        Some(Obj::File(fd)) => file_ov_io(*fd, true, buf, n as usize, ov, routine),
        Some(Obj::Std(fd)) => {
            unsafe { libc::write(*fd, buf as *const _, n as usize) };
            let tid = cur_tid();
            queue_apc(tid, Box::new(move || {
                let f: Completion = unsafe { std::mem::transmute(routine) };
                f(0, n, ov);
            }));
            TRUE
        }
        _ => {
            log!(1, "WriteFileEx on unsupported handle {h:#x}");
            set_err(6);
            FALSE
        }
    }
}
pub extern "win64" fn CancelIo(h: usize) -> usize {
    if let Some(Obj::Comm(c)) = get_handle(h).as_deref() {
        c.cancel();
    }
    TRUE
}

fn fd_of(h: usize) -> Option<i32> {
    match get_handle(h).as_deref() {
        Some(Obj::File(fd)) => Some(*fd),
        _ => None,
    }
}
pub extern "win64" fn GetFileSize(h: usize, high: usize) -> u32 {
    let Some(fd) = fd_of(h) else { return 0xffff_ffff };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    unsafe { libc::fstat(fd, &mut st) };
    if high != 0 {
        unsafe { w32(high, (st.st_size as u64 >> 32) as u32) };
    }
    st.st_size as u32
}
pub extern "win64" fn SetFilePointer(h: usize, lo: i32, high: usize, method: u32) -> u32 {
    let Some(fd) = fd_of(h) else { set_err(6); return 0xffff_ffff };
    let off = if high != 0 { ((unsafe { r32(high) } as i64) << 32) | (lo as u32 as i64) } else { lo as i64 };
    let r = unsafe { libc::lseek(fd, off, method as i32) };
    if high != 0 {
        unsafe { w32(high, (r as u64 >> 32) as u32) };
    }
    r as u32
}
pub extern "win64" fn SetFilePointerEx(h: usize, dist: i64, newpos: usize, method: u32) -> usize {
    let Some(fd) = fd_of(h) else { set_err(6); return FALSE };
    let r = unsafe { libc::lseek(fd, dist, method as i32) };
    if newpos != 0 {
        unsafe { w64(newpos, r as u64) };
    }
    (r >= 0) as usize
}
pub extern "win64" fn SetEndOfFile(h: usize) -> usize {
    let Some(fd) = fd_of(h) else { return FALSE };
    unsafe {
        let p = libc::lseek(fd, 0, libc::SEEK_CUR);
        (libc::ftruncate(fd, p) == 0) as usize
    }
}
pub extern "win64" fn FlushFileBuffers(h: usize) -> usize {
    if let Some(fd) = fd_of(h) {
        unsafe { libc::fsync(fd) };
    }
    TRUE
}
pub extern "win64" fn GetFileType(h: usize) -> u32 {
    match get_handle(h).or_else(|| std_handle(h)).as_deref() {
        Some(Obj::File(_)) => 1,
        Some(_) => 2,
        None => 0,
    }
}
pub extern "win64" fn GetFileInformationByHandle(h: usize, info: usize) -> usize {
    let Some(fd) = fd_of(h) else { set_err(6); return FALSE };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    unsafe {
        libc::fstat(fd, &mut st);
        zero(info, 52);
        w32(info, 0x80);
        let ft = unix_to_filetime(st.st_mtime as i64, 0);
        for o in [4, 12, 20] {
            w64(info + o, ft);
        }
        w32(info + 32, (st.st_size as u64 >> 32) as u32);
        w32(info + 36, st.st_size as u32);
        w32(info + 40, 1);
    }
    TRUE
}
pub extern "win64" fn DeleteFileA(p: usize) -> usize {
    std::fs::remove_file(to_unix(&unsafe { astr(p) })).is_ok() as usize
}
pub extern "win64" fn DeleteFileW(p: usize) -> usize {
    std::fs::remove_file(to_unix(&unsafe { wstr(p) })).is_ok() as usize
}
pub extern "win64" fn MoveFileA(a: usize, b: usize) -> usize {
    std::fs::rename(to_unix(&unsafe { astr(a) }), to_unix(&unsafe { astr(b) })).is_ok() as usize
}
fn mkdir(p: String) -> usize {
    let u = to_unix(&p);
    if std::path::Path::new(&u).is_dir() {
        set_err(183);
        return FALSE;
    }
    std::fs::create_dir_all(&u).is_ok() as usize
}
pub extern "win64" fn CreateDirectoryA(p: usize, _sa: usize) -> usize {
    mkdir(unsafe { astr(p) })
}
pub extern "win64" fn CreateDirectoryW(p: usize, _sa: usize) -> usize {
    mkdir(unsafe { wstr(p) })
}
pub extern "win64" fn GetFileAttributesA(p: usize) -> u32 {
    match std::fs::metadata(to_unix(&unsafe { astr(p) })) {
        Ok(m) if m.is_dir() => 0x10,
        Ok(_) => 0x80,
        Err(_) => {
            set_err(2);
            0xffff_ffff
        }
    }
}
pub extern "win64" fn FindFirstFileA(_p: usize, _d: usize) -> usize {
    set_err(2);
    INVALID
}
pub extern "win64" fn FindFirstFileExW(_p: usize, _l: u32, _d: usize, _s: u32, _f: usize, _fl: u32) -> usize {
    set_err(2);
    INVALID
}
pub extern "win64" fn FindNextFileA(_h: usize, _d: usize) -> usize {
    set_err(18);
    FALSE
}
pub extern "win64" fn FindClose(_h: usize) -> usize {
    TRUE
}
pub extern "win64" fn GetTempPathA(n: u32, buf: usize) -> u32 {
    let _ = std::fs::create_dir_all(format!("{}/tmp", cfg().root));
    unsafe { put_astr(buf, n as usize, "C:\\tmp\\") as u32 }
}
pub extern "win64" fn GetTempFileNameA(path: usize, prefix: usize, unique: u32, out: usize) -> u32 {
    let u = if unique != 0 { unique } else { (std::process::id() ^ now_ms() as u32) & 0xffff };
    let pre: String = unsafe { astr(prefix) }.chars().take(3).collect();
    let mut dir = unsafe { astr(path) };
    if !dir.ends_with('\\') {
        dir.push('\\');
    }
    let name = format!("{dir}{pre}{u:04X}.tmp");
    let _ = std::fs::write(to_unix(&name), b"");
    unsafe { put_astr(out, 260, &name) };
    u
}
pub extern "win64" fn GetFullPathNameW(p: usize, n: u32, buf: usize, filepart: usize) -> u32 {
    let mut s = unsafe { wstr(p) };
    if !(s.len() > 1 && s.as_bytes()[1] == b':') {
        s = format!("C:\\{s}");
    }
    if filepart != 0 {
        unsafe { w64(filepart, 0) };
    }
    unsafe { put_wstr(buf, n as usize, &s) as u32 }
}
pub extern "win64" fn GetCurrentDirectoryW(n: u32, buf: usize) -> u32 {
    unsafe { put_wstr(buf, n as usize, "C:\\") as u32 }
}
pub extern "win64" fn GetDriveTypeW(_p: usize) -> u32 {
    3
}
pub extern "win64" fn PeekNamedPipe(_h: usize, _b: usize, _n: u32, _r: usize, avail: usize, _l: usize) -> usize {
    if avail != 0 {
        unsafe { w32(avail, 0) };
    }
    FALSE
}

// ---- serial port ------------------------------------------------------------------------------
fn comm_of(h: usize) -> Option<Arc<Comm>> {
    match get_handle(h).as_deref() {
        Some(Obj::Comm(c)) => Some(c.clone()),
        _ => None,
    }
}
pub extern "win64" fn SetupComm(_h: usize, _i: u32, _o: u32) -> usize {
    TRUE
}
pub extern "win64" fn PurgeComm(h: usize, flags: u32) -> usize {
    if let Some(c) = comm_of(h) {
        if flags & (0x2 | 0x8) != 0 {
            c.cancel();
        }
        if flags & 0x8 != 0 {
            c.purge_rx();
        }
    }
    TRUE
}
pub extern "win64" fn ClearCommError(h: usize, errs: usize, stat: usize) -> usize {
    let Some(c) = comm_of(h) else { return if errs != 0 || stat != 0 { TRUE } else { FALSE } };
    unsafe {
        if errs != 0 {
            w32(errs, 0);
        }
        if stat != 0 {
            w32(stat, 0);
            w32(stat + 4, c.available() as u32);
            w32(stat + 8, 0);
        }
    }
    TRUE
}
pub extern "win64" fn GetCommState(h: usize, dcb: usize) -> usize {
    let Some(c) = comm_of(h) else { set_err(6); return FALSE };
    unsafe {
        let len = r32(dcb).max(28);
        zero(dcb, len.min(28) as usize);
        w32(dcb, len);
        w32(dcb + 4, c.baud.load(Ordering::Relaxed));
        w32(dcb + 8, 1 | (1 << 2) | (2 << 12)); // fBinary, fOutxCtsFlow, fRtsControl=HANDSHAKE
        w8(dcb + 18, 8);
    }
    TRUE
}
pub extern "win64" fn SetCommState(h: usize, dcb: usize) -> usize {
    let Some(c) = comm_of(h) else { set_err(6); return FALSE };
    let baud = unsafe { r32(dcb + 4) };
    log!(1, "SetCommState baud={baud} flags={:#x}", unsafe { r32(dcb + 8) });
    c.set_baud(baud);
    TRUE
}
pub extern "win64" fn BuildCommDCBA(def: usize, dcb: usize) -> usize {
    let s = unsafe { astr(def) }.to_ascii_lowercase();
    log!(1, "BuildCommDCBA({s})");
    let baud = s
        .split(|ch: char| ch == ' ' || ch == ',')
        .find_map(|t| t.strip_prefix("baud=").and_then(|v| v.parse::<u32>().ok()))
        .or_else(|| s.split(|ch: char| ch == ':' || ch == ',').find_map(|t| t.trim().parse::<u32>().ok()));
    unsafe {
        if let Some(b) = baud {
            w32(dcb + 4, b);
        }
        w8(dcb + 18, 8);
    }
    TRUE
}
pub extern "win64" fn SetCommTimeouts(h: usize, t: usize) -> usize {
    let Some(c) = comm_of(h) else { set_err(6); return FALSE };
    let v = unsafe { crate::comm::Timeouts { ri: r32(t), rm: r32(t + 4), rc: r32(t + 8), wm: r32(t + 12), wc: r32(t + 16) } };
    log!(1, "SetCommTimeouts ri={} rm={} rc={}", v.ri, v.rm, v.rc);
    *c.timeouts.lock().unwrap() = v;
    TRUE
}
pub extern "win64" fn EscapeCommFunction(_h: usize, f: u32) -> usize {
    log!(2, "EscapeCommFunction({f})");
    TRUE
}

// ---- GPIO (replacement for BcmGnssGpioAcpi.dll / Gpio.dll) -------------------------------------
static GPIO_STATE: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
pub extern "win64" fn GpioDllInit(ctx: usize, port: usize) -> u32 {
    if ctx != 0 {
        unsafe { w64(ctx, port as u64) };
    }
    0
}
pub extern "win64" fn GpioDllDeinit(_ctx: usize) -> u32 {
    0
}
pub extern "win64" fn GpioGet(_ctx: usize, pin: u32, val: usize) -> u32 {
    if pin > 1 {
        return 1;
    }
    unsafe { w32(val, GPIO_STATE[pin as usize].load(Ordering::SeqCst)) };
    0
}
pub extern "win64" fn GpioSet(_ctx: usize, pin: u32, val: u32) -> u32 {
    if pin > 1 {
        return 1;
    }
    let v = (val != 0) as u32;
    GPIO_STATE[pin as usize].store(v, Ordering::SeqCst);
    log!(1, "GPIO {} -> {v}", if pin == 1 { "REGPU" } else { "RESET (not wired)" });
    if pin == 1 {
        if let Err(e) = std::fs::write(&cfg().comm.regpu, if v != 0 { "1" } else { "0" }) {
            log!(0, "GPIO: cannot write {}: {e}", cfg().comm.regpu);
        }
    }
    0
}

// ---- sync -------------------------------------------------------------------------------------
pub extern "win64" fn CreateEventA(_sa: usize, manual: i32, initial: i32, _name: usize) -> usize {
    new_handle(Obj::Event(Arc::new(Event { manual: manual != 0, set: AtomicBool::new(initial != 0) })))
}
fn event_op(h: usize, v: bool) -> usize {
    match get_handle(h).as_deref() {
        Some(Obj::Event(e)) => {
            e.set.store(v, Ordering::SeqCst);
            if v {
                notify();
            }
            TRUE
        }
        _ => {
            set_err(6);
            FALSE
        }
    }
}
pub extern "win64" fn SetEvent(h: usize) -> usize {
    event_op(h, true)
}
pub extern "win64" fn ResetEvent(h: usize) -> usize {
    event_op(h, false)
}
pub extern "win64" fn WaitForSingleObject(h: usize, ms: u32) -> u32 {
    wait(&[h], false, ms, false)
}
pub extern "win64" fn WaitForSingleObjectEx(h: usize, ms: u32, alertable: i32) -> u32 {
    wait(&[h], false, ms, alertable != 0)
}
fn handles_at(n: u32, p: usize) -> Vec<usize> {
    (0..n as usize).map(|i| unsafe { r64(p + i * 8) as usize }).collect()
}
pub extern "win64" fn WaitForMultipleObjects(n: u32, p: usize, all: i32, ms: u32) -> u32 {
    wait(&handles_at(n, p), all != 0, ms, false)
}
pub extern "win64" fn WaitForMultipleObjectsEx(n: u32, p: usize, all: i32, ms: u32, alertable: i32) -> u32 {
    wait(&handles_at(n, p), all != 0, ms, alertable != 0)
}
pub extern "win64" fn Sleep(ms: u32) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}
pub extern "win64" fn SleepEx(ms: u32, alertable: i32) -> u32 {
    if alertable == 0 {
        Sleep(ms);
        return 0;
    }
    match wait(&[], false, ms, true) {
        WAIT_IO_COMPLETION => WAIT_IO_COMPLETION,
        _ => 0,
    }
}

// critical sections: keyed by the address of the caller's CRITICAL_SECTION
struct RecMutex {
    m: std::sync::Mutex<(u32, u32)>,
    cv: std::sync::Condvar,
}
static CS: OnceLock<std::sync::Mutex<std::collections::HashMap<usize, Arc<RecMutex>>>> = OnceLock::new();
fn cs(p: usize) -> Arc<RecMutex> {
    CS.get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(p)
        .or_insert_with(|| Arc::new(RecMutex { m: std::sync::Mutex::new((0, 0)), cv: std::sync::Condvar::new() }))
        .clone()
}
pub extern "win64" fn InitializeCriticalSection(p: usize) {
    unsafe { zero(p, 40) };
    cs(p);
}
pub extern "win64" fn InitializeCriticalSectionAndSpinCount(p: usize, _spin: u32) -> usize {
    InitializeCriticalSection(p);
    TRUE
}
pub extern "win64" fn InitializeCriticalSectionEx(p: usize, _spin: u32, _f: u32) -> usize {
    InitializeCriticalSection(p);
    TRUE
}
pub extern "win64" fn DeleteCriticalSection(p: usize) {
    CS.get_or_init(Default::default).lock().unwrap().remove(&p);
}
pub extern "win64" fn EnterCriticalSection(p: usize) {
    let c = cs(p);
    let me = cur_tid();
    let mut g = c.m.lock().unwrap();
    while g.1 > 0 && g.0 != me {
        g = c.cv.wait(g).unwrap();
    }
    g.0 = me;
    g.1 += 1;
}
pub extern "win64" fn LeaveCriticalSection(p: usize) {
    let c = cs(p);
    let mut g = c.m.lock().unwrap();
    if g.1 > 0 {
        g.1 -= 1;
        if g.1 == 0 {
            g.0 = 0;
            c.cv.notify_one();
        }
    }
}

// ---- threads & process ------------------------------------------------------------------------
pub extern "win64" fn CreateThread(_sa: usize, stack: usize, start: usize, param: usize, flags: u32, tid: usize) -> usize {
    let (h, t) = spawn(start, param, flags & 4 != 0, stack);
    if tid != 0 {
        unsafe { w32(tid, t) };
    }
    log!(1, "CreateThread({start:#x}) -> tid {t:x}");
    h
}
pub extern "win64" fn ResumeThread(h: usize) -> u32 {
    if let Some(Obj::Thread(t)) = get_handle(h).as_deref() {
        let was = t.suspended.swap(false, Ordering::SeqCst);
        notify();
        return was as u32;
    }
    0xffff_ffff
}
pub extern "win64" fn ExitThread(code: u32) -> ! {
    log!(1, "ExitThread({code})");
    if let Some(t) = current_thread_obj() {
        finish_thread(&t, code);
    }
    loop {
        std::thread::park();
    }
}
pub extern "win64" fn ExitProcess(code: u32) -> ! {
    log!(0, "ExitProcess({code})");
    std::process::exit(code as i32)
}
pub extern "win64" fn TerminateProcess(_h: usize, code: u32) -> usize {
    log!(0, "TerminateProcess({code})");
    std::process::exit(code as i32)
}
pub extern "win64" fn GetCurrentThreadId() -> u32 {
    cur_tid()
}
pub extern "win64" fn GetCurrentProcessId() -> u32 {
    std::process::id()
}
pub extern "win64" fn GetCurrentProcess() -> usize {
    INVALID
}
pub extern "win64" fn CreateProcessA(_a: usize, cmd: usize, _c: usize, _d: usize, _e: i32, _f: u32, _g: usize, _h: usize, _i: usize, _j: usize) -> usize {
    log!(0, "CreateProcessA({}) refused", unsafe { astr(cmd) });
    set_err(2);
    FALSE
}
pub extern "win64" fn GetProcessTimes(_h: usize, a: usize, b: usize, c: usize, d: usize) -> usize {
    for p in [a, b, c, d] {
        if p != 0 {
            unsafe { w64(p, 0) };
        }
    }
    TRUE
}
pub extern "win64" fn SetPriorityClass(_h: usize, _c: u32) -> usize {
    TRUE
}

// ---- TLS / FLS --------------------------------------------------------------------------------
static NEXT_TLS: AtomicU32 = AtomicU32::new(1);
pub extern "win64" fn TlsAlloc() -> u32 {
    NEXT_TLS.fetch_add(1, Ordering::SeqCst)
}
pub extern "win64" fn TlsFree(_i: u32) -> usize {
    TRUE
}
pub extern "win64" fn TlsGetValue(i: u32) -> usize {
    set_err(0);
    tls_get(i as usize)
}
pub extern "win64" fn TlsSetValue(i: u32, v: usize) -> usize {
    tls_set(i as usize, v) as usize
}

// ---- errors, exceptions -----------------------------------------------------------------------
pub extern "win64" fn GetLastError() -> u32 {
    get_err()
}
pub extern "win64" fn SetLastError(e: u32) {
    set_err(e)
}
pub extern "win64" fn RaiseException(code: u32, flags: u32, n: u32, args: usize) {
    if code == 0x406D_1388 {
        return; // MSVC "set thread name"
    }
    let a: Vec<String> = (0..n.min(4) as usize).map(|i| format!("{:#x}", unsafe { r64(args + i * 8) })).collect();
    eprintln!("tabpro-s-gnss: RaiseException code={code:#x} flags={flags:#x} args={a:?} — engine stopped");
    crate::shutdown_engine();
    std::process::exit(4);
}
pub extern "win64" fn SetUnhandledExceptionFilter(_f: usize) -> usize {
    0
}
pub extern "win64" fn UnhandledExceptionFilter(_e: usize) -> i32 {
    0
}
pub extern "win64" fn IsDebuggerPresent() -> usize {
    FALSE
}
pub extern "win64" fn RtlCaptureContext(ctx: usize) {
    unsafe { zero(ctx, 0x4d0) };
}
pub extern "win64" fn RtlLookupFunctionEntry(_pc: usize, base: usize, _h: usize) -> usize {
    if base != 0 {
        unsafe { w64(base, 0) };
    }
    0
}
pub extern "win64" fn RtlPcToFileHeader(pc: usize, base: usize) -> usize {
    let r = &cfg().image;
    let b = if r.contains(&pc) { r.start } else { 0 };
    unsafe { w64(base, b as u64) };
    b
}
pub extern "win64" fn RtlVirtualUnwind() -> usize {
    eprintln!("tabpro-s-gnss: RtlVirtualUnwind (C++ exception) — not supported, aborting");
    std::process::abort();
}
pub extern "win64" fn RtlUnwindEx() -> usize {
    eprintln!("tabpro-s-gnss: RtlUnwindEx (C++ exception) — not supported, aborting");
    std::process::abort();
}
pub extern "win64" fn EncodePointer(p: usize) -> usize {
    p
}
pub extern "win64" fn DecodePointer(p: usize) -> usize {
    p
}
pub extern "win64" fn IsProcessorFeaturePresent(f: u32) -> usize {
    matches!(f, 6 | 10 | 13) as usize // XMMI, XMMI64 (SSE2), SSE3
}
pub extern "win64" fn FormatMessageA(_f: u32, _s: usize, _m: u32, _l: u32, buf: usize, n: u32, _a: usize) -> u32 {
    if buf != 0 && n > 0 {
        unsafe { w8(buf, 0) };
    }
    0
}
pub extern "win64" fn OutputDebugStringA(s: usize) {
    log!(1, "debug: {}", unsafe { astr(s) }.trim_end());
}
pub extern "win64" fn OutputDebugStringW(s: usize) {
    log!(1, "debug: {}", unsafe { wstr(s) }.trim_end());
}

// ---- heap -------------------------------------------------------------------------------------
pub extern "win64" fn GetProcessHeap() -> usize {
    HEAP_HANDLE
}
pub extern "win64" fn HeapAlloc(_h: usize, flags: u32, n: usize) -> usize {
    let n = n.max(1);
    (unsafe { if flags & 8 != 0 { libc::calloc(1, n) } else { libc::malloc(n) } }) as usize
}
pub extern "win64" fn HeapFree(_h: usize, _f: u32, p: usize) -> usize {
    unsafe { libc::free(p as *mut _) };
    TRUE
}
pub extern "win64" fn HeapReAlloc(_h: usize, flags: u32, p: usize, n: usize) -> usize {
    unsafe {
        let old = libc::malloc_usable_size(p as *mut _);
        let q = libc::realloc(p as *mut _, n.max(1)) as usize;
        if flags & 8 != 0 && q != 0 && n > old {
            zero(q + old, n - old);
        }
        q
    }
}
pub extern "win64" fn HeapSize(_h: usize, _f: u32, p: usize) -> usize {
    unsafe { libc::malloc_usable_size(p as *mut _) }
}
pub extern "win64" fn LocalFree(p: usize) -> usize {
    unsafe { libc::free(p as *mut _) };
    0
}
pub extern "win64" fn GlobalMemoryStatus(p: usize) {
    unsafe {
        zero(p, 56);
        w32(p, 56);
        w32(p + 4, 50);
        for (i, v) in [4u64 << 30, 2 << 30, 8 << 30, 6 << 30, 128 << 40, 127 << 40].iter().enumerate() {
            w64(p + 8 + i * 8, *v);
        }
    }
}

// ---- time -------------------------------------------------------------------------------------
const EPOCH_DIFF: u64 = 116_444_736_000_000_000; // 1601 -> 1970 in 100 ns
fn now_ms() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    t.as_millis() as u64
}
fn unix_to_filetime(secs: i64, nanos: u32) -> u64 {
    (secs as u64) * 10_000_000 + (nanos / 100) as u64 + EPOCH_DIFF
}
fn now_filetime() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    unix_to_filetime(t.as_secs() as i64, t.subsec_nanos())
}
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
unsafe fn ft_to_st(ft: u64, st: usize) {
    let t = (ft - EPOCH_DIFF) / 10_000;
    let (ms, secs) = (t % 1000, (t / 1000) as i64);
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let dow = (days + 4).rem_euclid(7);
    for (i, v) in [y as u16, m as u16, dow as u16, d as u16, (sod / 3600) as u16, (sod / 60 % 60) as u16, (sod % 60) as u16, ms as u16]
        .iter()
        .enumerate()
    {
        w16(st + i * 2, *v);
    }
}
pub extern "win64" fn GetSystemTimeAsFileTime(p: usize) {
    unsafe { w64(p, now_filetime()) };
}
pub extern "win64" fn GetSystemTime(st: usize) {
    unsafe { ft_to_st(now_filetime(), st) };
}
pub extern "win64" fn GetLocalTime(st: usize) {
    unsafe { ft_to_st(now_filetime(), st) };
}
pub extern "win64" fn FileTimeToSystemTime(ft: usize, st: usize) -> usize {
    unsafe { ft_to_st(r64(ft), st) };
    TRUE
}
pub extern "win64" fn SystemTimeToFileTime(st: usize, ft: usize) -> usize {
    unsafe {
        let g = |i: usize| (st as *const u16).add(i).read_unaligned() as i64;
        let days = days_from_civil(g(0), g(1) as u32, g(3) as u32);
        let secs = days * 86400 + g(4) * 3600 + g(5) * 60 + g(6);
        w64(ft, secs as u64 * 10_000_000 + g(7) as u64 * 10_000 + EPOCH_DIFF);
    }
    TRUE
}
pub extern "win64" fn FileTimeToLocalFileTime(a: usize, b: usize) -> usize {
    unsafe { w64(b, r64(a)) };
    TRUE
}
pub extern "win64" fn LocalFileTimeToFileTime(a: usize, b: usize) -> usize {
    unsafe { w64(b, r64(a)) };
    TRUE
}
pub extern "win64" fn GetTimeZoneInformation(p: usize) -> u32 {
    unsafe { zero(p, 172) };
    0
}
pub extern "win64" fn GetTickCount() -> u32 {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    (ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000) as u32
}
pub extern "win64" fn QueryPerformanceCounter(p: usize) -> usize {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        w64(p, ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64);
    }
    TRUE
}
pub extern "win64" fn QueryPerformanceFrequency(p: usize) -> usize {
    unsafe { w64(p, 1_000_000_000) };
    TRUE
}

// ---- environment, version, console --------------------------------------------------------------
static CMDLINE: &[u8] = b"tabpro-s-gnss.exe\0";
static ENV_W: [u16; 2] = [0, 0];
pub extern "win64" fn GetCommandLineA() -> usize {
    CMDLINE.as_ptr() as usize
}
pub extern "win64" fn GetEnvironmentStringsW() -> usize {
    ENV_W.as_ptr() as usize
}
pub extern "win64" fn FreeEnvironmentStringsW(_p: usize) -> usize {
    TRUE
}
pub extern "win64" fn SetEnvironmentVariableA(_n: usize, _v: usize) -> usize {
    TRUE
}
pub extern "win64" fn GetStartupInfoW(p: usize) {
    unsafe {
        zero(p, 104);
        w32(p, 104);
    }
}
pub extern "win64" fn GetVersion() -> u32 {
    (19041 << 16) | 10
}
pub extern "win64" fn GetVersionExA(p: usize) -> usize {
    unsafe {
        w32(p + 4, 10);
        w32(p + 8, 0);
        w32(p + 12, 19041);
        w32(p + 16, 2);
        w8(p + 20, 0);
    }
    TRUE
}
fn std_handle(h: usize) -> Option<Arc<Obj>> {
    match h as isize {
        -10 => Some(Arc::new(Obj::Std(0))),
        -11 => Some(Arc::new(Obj::Std(1))),
        -12 => Some(Arc::new(Obj::Std(2))),
        _ => None,
    }
}
pub extern "win64" fn GetStdHandle(n: u32) -> usize {
    n as i32 as isize as usize // pseudo handles -10/-11/-12, resolved by std_handle()
}
pub extern "win64" fn SetStdHandle(_n: u32, _h: usize) -> usize {
    TRUE
}
pub extern "win64" fn GetConsoleMode(_h: usize, _m: usize) -> usize {
    FALSE
}
pub extern "win64" fn SetConsoleMode(_h: usize, _m: u32) -> usize {
    TRUE
}
pub extern "win64" fn GetConsoleCP() -> u32 {
    437
}
pub extern "win64" fn WriteConsoleW(_h: usize, s: usize, n: u32, w: usize, _r: usize) -> usize {
    let v = unsafe { std::slice::from_raw_parts(s as *const u16, n as usize) };
    eprint!("{}", String::from_utf16_lossy(v));
    if w != 0 {
        unsafe { w32(w, n) };
    }
    TRUE
}
pub extern "win64" fn ReadConsoleW(_h: usize, _b: usize, _n: u32, r: usize, _c: usize) -> usize {
    if r != 0 {
        unsafe { w32(r, 0) };
    }
    FALSE
}
pub extern "win64" fn GetNumberOfConsoleInputEvents(_h: usize, n: usize) -> usize {
    unsafe { w32(n, 0) };
    TRUE
}
pub extern "win64" fn PeekConsoleInputA(_h: usize, _b: usize, _l: u32, r: usize) -> usize {
    unsafe { w32(r, 0) };
    TRUE
}
pub extern "win64" fn ReadConsoleInputA(_h: usize, _b: usize, _l: u32, r: usize) -> usize {
    std::thread::sleep(std::time::Duration::from_millis(500));
    unsafe { w32(r, 0) };
    TRUE
}
pub extern "win64" fn FlushConsoleInputBuffer(_h: usize) -> usize {
    TRUE
}
pub extern "win64" fn SetConsoleCtrlHandler(_f: usize, _a: i32) -> usize {
    TRUE
}

// ---- modules ----------------------------------------------------------------------------------
pub extern "win64" fn LoadLibraryA(name: usize) -> usize {
    crate::modules::load_library(&unsafe { astr(name) })
}
pub extern "win64" fn LoadLibraryW(name: usize) -> usize {
    crate::modules::load_library(&unsafe { wstr(name) })
}
pub extern "win64" fn LoadLibraryExW(name: usize, _f: usize, _fl: u32) -> usize {
    crate::modules::load_library(&unsafe { wstr(name) })
}
pub extern "win64" fn FreeLibrary(_h: usize) -> usize {
    TRUE
}
pub extern "win64" fn GetProcAddress(h: usize, name: usize) -> usize {
    let n = if name < 0x10000 { format!("#{name}") } else { unsafe { astr(name) } };
    crate::modules::get_proc(h, &n)
}
pub extern "win64" fn GetModuleHandleA(name: usize) -> usize {
    crate::modules::module_handle(if name == 0 { None } else { Some(unsafe { astr(name) }) })
}
pub extern "win64" fn GetModuleHandleW(name: usize) -> usize {
    crate::modules::module_handle(if name == 0 { None } else { Some(unsafe { wstr(name) }) })
}
pub extern "win64" fn GetModuleHandleExW(flags: u32, name: usize, out: usize) -> usize {
    let h = if flags & 4 != 0 {
        if cfg().image.contains(&name) { cfg().image.start } else { 0 }
    } else {
        GetModuleHandleW(name)
    };
    unsafe { w64(out, h as u64) };
    (h != 0) as usize
}
fn module_name(h: usize) -> &'static str {
    if h == cfg().image.start { "C:\\BcmGnss.dll" } else { "C:\\tabpro-s-gnss.exe" }
}
pub extern "win64" fn GetModuleFileNameA(h: usize, buf: usize, n: u32) -> u32 {
    unsafe { put_astr(buf, n as usize, module_name(h)) as u32 }
}
pub extern "win64" fn GetModuleFileNameW(h: usize, buf: usize, n: u32) -> u32 {
    unsafe { put_wstr(buf, n as usize, module_name(h)) as u32 }
}
pub extern "win64" fn DisableThreadLibraryCalls(_h: usize) -> usize {
    TRUE
}

// ---- strings & locale (C locale only) -----------------------------------------------------------
pub extern "win64" fn GetACP() -> u32 {
    1252
}
pub extern "win64" fn GetOEMCP() -> u32 {
    437
}
pub extern "win64" fn IsValidCodePage(_c: u32) -> usize {
    TRUE
}
pub extern "win64" fn AreFileApisANSI() -> usize {
    TRUE
}
pub extern "win64" fn GetCPInfo(_cp: u32, p: usize) -> usize {
    unsafe {
        zero(p, 20);
        w32(p, 1);
        w8(p + 4, b'?');
    }
    TRUE
}
pub extern "win64" fn MultiByteToWideChar(_cp: u32, _f: u32, src: usize, n: i32, dst: usize, cap: i32) -> i32 {
    let bytes: &[u8] = unsafe {
        if n < 0 { std::ffi::CStr::from_ptr(src as *const _).to_bytes_with_nul() } else { std::slice::from_raw_parts(src as *const u8, n as usize) }
    };
    let v: Vec<u16> = String::from_utf8_lossy(bytes).encode_utf16().collect();
    if cap == 0 {
        return v.len() as i32;
    }
    if v.len() > cap as usize {
        set_err(122);
        return 0;
    }
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), dst as *mut u16, v.len()) };
    v.len() as i32
}
pub extern "win64" fn WideCharToMultiByte(_cp: u32, _f: u32, src: usize, n: i32, dst: usize, cap: i32, _d: usize, used: usize) -> i32 {
    let len = if n < 0 { (unsafe { wlen(src) }) + 1 } else { n as usize };
    let v = unsafe { std::slice::from_raw_parts(src as *const u16, len) };
    let s = String::from_utf16_lossy(v);
    let b = s.as_bytes();
    if used != 0 {
        unsafe { w32(used, 0) };
    }
    if cap == 0 {
        return b.len() as i32;
    }
    if b.len() > cap as usize {
        set_err(122);
        return 0;
    }
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), dst as *mut u8, b.len()) };
    b.len() as i32
}
pub extern "win64" fn GetStringTypeW(_t: u32, src: usize, n: i32, out: usize) -> usize {
    let len = if n < 0 { (unsafe { wlen(src) }) + 1 } else { n as usize };
    for i in 0..len {
        let c = unsafe { *((src as *const u16).add(i)) };
        let mut f = 0x200u16;
        if c < 128 {
            let ch = c as u8 as char;
            if ch.is_ascii_uppercase() { f |= 1 | 0x100; }
            if ch.is_ascii_lowercase() { f |= 2 | 0x100; }
            if ch.is_ascii_digit() { f |= 4; }
            if ch.is_ascii_whitespace() { f |= 8; }
            if ch.is_ascii_punctuation() { f |= 0x10; }
            if ch.is_ascii_control() { f |= 0x20; }
            if ch == ' ' || ch == '\t' { f |= 0x40; }
            if ch.is_ascii_hexdigit() { f |= 0x80; }
        } else {
            f |= 0x100;
        }
        unsafe { w16(out + i * 2, f) };
    }
    TRUE
}
pub extern "win64" fn LCMapStringW(_l: u32, flags: u32, src: usize, n: i32, dst: usize, cap: i32) -> i32 {
    let len = if n < 0 { (unsafe { wlen(src) }) + 1 } else { n as usize };
    if flags & 0x400 != 0 {
        return 0; // sort keys unsupported
    }
    if cap == 0 {
        return len as i32;
    }
    for i in 0..len.min(cap as usize) {
        let mut c = unsafe { *((src as *const u16).add(i)) };
        if c < 128 {
            if flags & 0x100 != 0 { c = (c as u8).to_ascii_lowercase() as u16; }
            if flags & 0x200 != 0 { c = (c as u8).to_ascii_uppercase() as u16; }
        }
        unsafe { w16(dst + i * 2, c) };
    }
    len as i32
}
pub extern "win64" fn CompareStringW(_l: u32, _f: u32, a: usize, na: i32, b: usize, nb: i32) -> i32 {
    let s = |p: usize, n: i32| unsafe { std::slice::from_raw_parts(p as *const u16, if n < 0 { wlen(p) } else { n as usize }).to_vec() };
    match s(a, na).cmp(&s(b, nb)) {
        std::cmp::Ordering::Less => 1,
        std::cmp::Ordering::Equal => 2,
        std::cmp::Ordering::Greater => 3,
    }
}
pub extern "win64" fn GetLocaleInfoW(_l: u32, _t: u32, _b: usize, _n: i32) -> i32 {
    0
}
pub extern "win64" fn IsValidLocale(_l: u32, _f: u32) -> usize {
    TRUE
}
pub extern "win64" fn GetUserDefaultLCID() -> u32 {
    0x409
}
pub extern "win64" fn EnumSystemLocalesW(_f: usize, _fl: u32) -> usize {
    TRUE
}
pub extern "win64" fn GetTimeFormatW(_l: u32, _f: u32, _t: usize, _fmt: usize, _b: usize, _n: i32) -> i32 {
    0
}
pub extern "win64" fn GetDateFormatW(_l: u32, _f: u32, _t: usize, _fmt: usize, _b: usize, _n: i32) -> i32 {
    0
}
pub extern "win64" fn lstrlenW(s: usize) -> i32 {
    if s == 0 { 0 } else { unsafe { wlen(s) as i32 } }
}
pub extern "win64" fn lstrcpyW(d: usize, s: usize) -> usize {
    let n = unsafe { wlen(s) } + 1;
    unsafe { std::ptr::copy(s as *const u16, d as *mut u16, n) };
    d
}
