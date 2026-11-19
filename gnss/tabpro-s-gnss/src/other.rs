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
// ---- fake NTP: UDP sockets are answered locally from the (chrony-synchronised) system clock ----
// The engine's SNTP client is its only source of absolute time; without it a 2015 GLL resolves the
// 10-bit GPS week into 1999-2019 (dates in 2007) and GLONASS positions go wrong.
struct UdpSock {
    pending: Option<[u8; 8]>, // transmit timestamp of the last request (echoed as originate)
}
static UDP: std::sync::Mutex<Vec<(usize, UdpSock)>> = std::sync::Mutex::new(Vec::new());
static NEXT_SOCK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0x7000);

fn ntp_now() -> [u8; 8] {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let secs = t.as_secs() + 2_208_988_800;
    let frac = ((t.subsec_nanos() as u64) << 32) / 1_000_000_000;
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&(secs as u32).to_be_bytes());
    b[4..].copy_from_slice(&(frac as u32).to_be_bytes());
    b
}

fn ntp_reply(orig: [u8; 8]) -> [u8; 48] {
    let mut r = [0u8; 48];
    r[0] = 0x24; // LI 0, version 4, mode 4 (server)
    r[1] = 2; // stratum
    r[2] = 6;
    r[3] = 0xec; // precision ~ 2^-20 s
    r[12..16].copy_from_slice(b"LOCL");
    let now = ntp_now();
    r[16..24].copy_from_slice(&now); // reference
    r[24..32].copy_from_slice(&orig); // originate
    r[32..40].copy_from_slice(&now); // receive
    r[40..48].copy_from_slice(&ntp_now()); // transmit
    r
}

fn udp_pending(s: usize) -> bool {
    UDP.lock().unwrap().iter().any(|(h, u)| *h == s && u.pending.is_some())
}

pub extern "win64" fn socket(af: i32, ty: i32, proto: i32) -> usize {
    if ty == 2 {
        let h = NEXT_SOCK.fetch_add(4, std::sync::atomic::Ordering::Relaxed);
        UDP.lock().unwrap().push((h, UdpSock { pending: None }));
        log!(1, "socket(UDP) -> local NTP responder {h:#x}");
        return h;
    }
    log!(1, "socket({af},{ty},{proto}) -> no network");
    sock_fail()
}
pub extern "win64" fn sendto(s: usize, buf: usize, len: i32, _f: i32, _to: usize, _tl: i32) -> i32 {
    let mut u = UDP.lock().unwrap();
    let Some((_, sock)) = u.iter_mut().find(|(h, _)| *h == s) else { drop(u); return sock_fail() as i32 };
    let b = unsafe { std::slice::from_raw_parts(buf as *const u8, len.max(0) as usize) };
    if b.len() >= 48 {
        let mut t = [0u8; 8];
        t.copy_from_slice(&b[40..48]);
        sock.pending = Some(t);
        log!(1, "NTP request -> answering from the system clock");
    }
    len
}
pub extern "win64" fn send(s: usize, buf: usize, len: i32, f: i32) -> i32 {
    sendto(s, buf, len, f, 0, 0)
}
pub extern "win64" fn recvfrom(s: usize, buf: usize, len: i32, _f: i32, from: usize, fromlen: usize) -> i32 {
    let mut u = UDP.lock().unwrap();
    let Some((_, sock)) = u.iter_mut().find(|(h, _)| *h == s) else { drop(u); return sock_fail() as i32 };
    let Some(orig) = sock.pending.take() else {
        WSA_ERR.with(|e| e.set(10035)); // WSAEWOULDBLOCK
        return -1;
    };
    let r = ntp_reply(orig);
    let n = r.len().min(len.max(0) as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(r.as_ptr(), buf as *mut u8, n);
        if from != 0 {
            zero(from, 16);
            w16(from, 2);
            w16(from + 2, 123u16.to_be());
            w32(from + 4, u32::from_ne_bytes([127, 0, 0, 1]));
            if fromlen != 0 {
                w32(fromlen, 16);
            }
        }
    }
    n as i32
}
pub extern "win64" fn recv(s: usize, buf: usize, len: i32, f: i32) -> i32 {
    recvfrom(s, buf, len, f, 0, 0)
}
/// select(): Windows fd_set = { u32 count; SOCKET fds[64] }
pub extern "win64" fn select(_n: i32, rd: usize, wr: usize, ex: usize, tv: usize) -> i32 {
    let filt = |set: usize, want: &dyn Fn(usize) -> bool| -> i32 {
        if set == 0 {
            return 0;
        }
        unsafe {
            let cnt = r32(set) as usize;
            let keep: Vec<u64> = (0..cnt.min(64)).map(|i| r64(set + 8 + i * 8)).filter(|h| want(*h as usize)).collect();
            w32(set, keep.len() as u32);
            for (i, h) in keep.iter().enumerate() {
                w64(set + 8 + i * 8, *h);
            }
            keep.len() as i32
        }
    };
    let is_udp = |h: usize| UDP.lock().unwrap().iter().any(|(x, _)| *x == h);
    let any_pending = |set: usize| set != 0 && unsafe { (0..(r32(set) as usize).min(64)).any(|i| udp_pending(r64(set + 8 + i * 8) as usize)) };
    if !any_pending(rd) && tv != 0 {
        let ms = unsafe { r32(tv) as u64 * 1000 + r32(tv + 4) as u64 / 1000 };
        std::thread::sleep(std::time::Duration::from_millis(ms.min(200)));
    }
    let n = filt(rd, &udp_pending) + filt(wr, &is_udp);
    filt(ex, &|_| false);
    n
}
pub extern "win64" fn sock_ok() -> i32 {
    0
}
static HOST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
/// hostent for 127.0.0.1 (only the local NTP responder is reachable)
fn local_hostent() -> usize {
    *HOST.get_or_init(|| {
        let addr: &'static [u8; 4] = Box::leak(Box::new([127, 0, 0, 1]));
        let list: &'static [usize; 2] = Box::leak(Box::new([addr.as_ptr() as usize, 0]));
        let aliases: &'static [usize; 1] = Box::leak(Box::new([0]));
        let name: &'static [u8] = b"localhost\0";
        let h = Box::leak(vec![0u8; 32].into_boxed_slice()).as_mut_ptr() as usize;
        unsafe {
            w64(h, name.as_ptr() as u64);
            w64(h + 8, aliases.as_ptr() as u64);
            w16(h + 16, 2);
            w16(h + 18, 4);
            w64(h + 24, list.as_ptr() as u64);
        }
        h
    })
}
pub extern "win64" fn sock_err() -> usize {
    sock_fail()
}
pub extern "win64" fn closesocket(_s: usize) -> i32 {
    0
}
pub extern "win64" fn getaddrinfo(node: usize, _s: usize, hints: usize, out: usize) -> i32 {
    let ty = if hints != 0 { unsafe { r32(hints + 8) } } else { 0 };
    if ty != 2 {
        log!(1, "getaddrinfo({}) -> not resolved (offline)", unsafe { astr(node) });
        if out != 0 {
            unsafe { w64(out, 0) };
        }
        return 11001;
    }
    log!(1, "getaddrinfo({}, UDP) -> local NTP responder", unsafe { astr(node) });
    // struct addrinfo { int flags, family, socktype, protocol; size_t addrlen; char *canon; sockaddr *addr; next }
    let ai = Box::leak(vec![0u8; 48].into_boxed_slice()).as_mut_ptr() as usize;
    let sa = Box::leak(vec![0u8; 16].into_boxed_slice()).as_mut_ptr() as usize;
    unsafe {
        w16(sa, 2);
        w16(sa + 2, 123u16.to_be());
        w32(sa + 4, u32::from_ne_bytes([127, 0, 0, 1]));
        w32(ai + 4, 2);
        w32(ai + 8, 2);
        w32(ai + 12, 17);
        w64(ai + 16, 16);
        w64(ai + 32, sa as u64);
        w64(out, ai as u64);
    }
    0
}
pub extern "win64" fn freeaddrinfo(_p: usize) {}
pub extern "win64" fn gethostbyname(n: usize) -> usize {
    log!(1, "gethostbyname({}) -> local NTP responder", unsafe { astr(n) });
    local_hostent()
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
