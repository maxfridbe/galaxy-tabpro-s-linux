//! ADVAPI32 / USER32 / ole32 / SHELL32 / WS2_32 / WINHTTP: stubs. The engine only needs networking
//! for assistance downloads (LTO/SUPL/NTP), which are disabled in the generated configuration.
#![allow(non_snake_case)]

use crate::log;
use crate::rt::*;
use std::cell::Cell;

const TRUE: usize = 1;
const FALSE: usize = 0;

// ---- registry: nothing stored; reads report "not found" ----------------------------------------
pub extern "win64" fn RegOpenKeyExA(_k: usize, sub: usize, _o: u32, _s: u32, out: usize) -> u32 {
    log!(2, "RegOpenKeyExA({}) -> not found", unsafe { astr(sub) });
    if out != 0 {
        unsafe { w64(out, 0) };
    }
    2
}
pub extern "win64" fn RegCreateKeyExA(_k: usize, sub: usize, _r: u32, _c: usize, _o: u32, _s: u32, _sa: usize, out: usize, disp: usize) -> u32 {
    log!(2, "RegCreateKeyExA({})", unsafe { astr(sub) });
    unsafe {
        w64(out, new_handle(Obj::Key) as u64);
        if disp != 0 {
            w32(disp, 1);
        }
    }
    0
}
pub extern "win64" fn RegQueryValueExA(_k: usize, name: usize, _r: usize, _t: usize, _d: usize, _n: usize) -> u32 {
    log!(2, "RegQueryValueExA({}) -> not found", unsafe { astr(name) });
    2
}
pub extern "win64" fn RegSetValueExA(_k: usize, _n: usize, _r: u32, _t: u32, _d: usize, _c: u32) -> u32 {
    0
}
pub extern "win64" fn RegDeleteValueA(_k: usize, _n: usize) -> u32 {
    0
}
pub extern "win64" fn RegDeleteKeyA(_k: usize, _n: usize) -> u32 {
    0
}
pub extern "win64" fn RegQueryInfoKeyA() -> u32 {
    2
}
pub extern "win64" fn RegCloseKey(k: usize) -> u32 {
    close_handle(k);
    0
}
pub extern "win64" fn RegisterEventSourceA(_s: usize, _n: usize) -> usize {
    0x5000
}
pub extern "win64" fn DeregisterEventSource(_h: usize) -> usize {
    TRUE
}
pub extern "win64" fn ReportEventA(_h: usize, t: u16, _c: u16, id: u32, _sid: usize, n: u16, _d: u32, strs: usize, _raw: usize) -> usize {
    let s: Vec<String> = (0..n as usize).map(|i| unsafe { astr(r64(strs + i * 8) as usize) }).collect();
    log!(0, "event log type={t} id={id:#x}: {}", s.join(" | "));
    TRUE
}

// ---- USER32 / SHELL32 / ole32 -------------------------------------------------------------------
pub extern "win64" fn MessageBoxA(_w: usize, text: usize, cap: usize, _t: u32) -> i32 {
    log!(0, "MessageBox [{}] {}", unsafe { astr(cap) }, unsafe { astr(text) });
    1
}
pub extern "win64" fn GetDesktopWindow() -> usize {
    0
}
pub extern "win64" fn GetProcessWindowStation() -> usize {
    0
}
pub extern "win64" fn GetUserObjectInformationW(_h: usize, _i: i32, _p: usize, _n: u32, _r: usize) -> usize {
    FALSE
}
pub extern "win64" fn SHGetSpecialFolderPathA(_w: usize, out: usize, csidl: i32, _create: i32) -> usize {
    log!(2, "SHGetSpecialFolderPathA({csidl:#x})");
    unsafe { put_astr(out, 260, "C:\\appdata") };
    TRUE
}
pub extern "win64" fn CoInitializeEx(_r: usize, _f: u32) -> u32 {
    0
}
pub extern "win64" fn CoUninitialize() {}
pub extern "win64" fn CoCreateInstance(_c: usize, _o: usize, _x: u32, _i: usize, out: usize) -> u32 {
    if out != 0 {
        unsafe { w64(out, 0) };
    }
    0x8000_4002
}
pub extern "win64" fn StringFromCLSID(_c: usize, out: usize) -> u32 {
    if out != 0 {
        unsafe { w64(out, 0) };
    }
    0x8000_4005
}

// ---- WS2_32: no network --------------------------------------------------------------------------
thread_local! { static WSA_ERR: Cell<i32> = const { Cell::new(0) }; }
const WSAENETDOWN: i32 = 10050;
fn sock_fail() -> usize {
    WSA_ERR.with(|e| e.set(WSAENETDOWN));
    usize::MAX
}
pub extern "win64" fn WSAStartup(ver: u16, data: usize) -> i32 {
    unsafe {
        zero(data, 408);
        w16(data, ver);
        w16(data + 2, 0x0202);
    }
    0
}
pub extern "win64" fn WSACleanup() -> i32 {
    0
}
pub extern "win64" fn WSAGetLastError() -> i32 {
    WSA_ERR.with(|e| e.get())
}
pub extern "win64" fn WSASetLastError(e: i32) {
    WSA_ERR.with(|c| c.set(e))
}
pub extern "win64" fn socket(af: i32, ty: i32, proto: i32) -> usize {
    log!(1, "socket({af},{ty},{proto}) -> no network");
    sock_fail()
}
pub extern "win64" fn sock_err() -> usize {
    sock_fail()
}
pub extern "win64" fn closesocket(_s: usize) -> i32 {
    0
}
pub extern "win64" fn getaddrinfo(node: usize, _s: usize, _h: usize, out: usize) -> i32 {
    log!(1, "getaddrinfo({}) -> not resolved (offline)", unsafe { astr(node) });
    if out != 0 {
        unsafe { w64(out, 0) };
    }
    11001
}
pub extern "win64" fn freeaddrinfo(_p: usize) {}
pub extern "win64" fn gethostbyname(n: usize) -> usize {
    log!(1, "gethostbyname({}) -> not resolved", unsafe { astr(n) });
    WSA_ERR.with(|e| e.set(11001));
    0
}
pub extern "win64" fn getprotobyname(_n: usize) -> usize {
    0
}
pub extern "win64" fn gethostname(buf: usize, n: i32) -> i32 {
    unsafe { put_astr(buf, n as usize, "tabpro-s") };
    0
}
pub extern "win64" fn htons(v: u16) -> u16 {
    v.to_be()
}
pub extern "win64" fn htonl(v: u32) -> u32 {
    v.to_be()
}
pub extern "win64" fn ntohl(v: u32) -> u32 {
    u32::from_be(v)
}
pub extern "win64" fn inet_addr(s: usize) -> u32 {
    unsafe { astr(s) }.parse::<std::net::Ipv4Addr>().map(|a| u32::from_ne_bytes(a.octets())).unwrap_or(u32::MAX)
}
static NTOA: std::sync::Mutex<[u8; 20]> = std::sync::Mutex::new([0; 20]);
pub extern "win64" fn inet_ntoa(a: u32) -> usize {
    let s = std::net::Ipv4Addr::from(a.to_ne_bytes()).to_string();
    let mut b = NTOA.lock().unwrap();
    b[..s.len()].copy_from_slice(s.as_bytes());
    b[s.len()] = 0;
    b.as_ptr() as usize
}
pub extern "win64" fn WSACreateEvent() -> usize {
    crate::k32::CreateEventA(0, 1, 0, 0)
}
pub extern "win64" fn WSACloseEvent(h: usize) -> usize {
    close_handle(h) as usize
}
pub extern "win64" fn WSAResetEvent(h: usize) -> usize {
    crate::k32::ResetEvent(h)
}

// ---- WINHTTP: offline ------------------------------------------------------------------------------
pub extern "win64" fn WinHttpOpen(agent: usize, _a: u32, _p: usize, _b: usize, _f: u32) -> usize {
    log!(1, "WinHttpOpen({}) -> offline", unsafe { wstr(agent) });
    set_err(12007);
    0
}
pub extern "win64" fn WinHttpFail() -> usize {
    set_err(12007);
    0
}
pub extern "win64" fn WinHttpCloseHandle(_h: usize) -> usize {
    TRUE
}
