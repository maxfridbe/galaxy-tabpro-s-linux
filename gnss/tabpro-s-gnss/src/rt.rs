//! Runtime core: memory helpers, handle table, per-thread TEB (via GS), threads, events, waits, APCs.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

// ---- memory helpers ----------------------------------------------------------------------------
pub unsafe fn w8(a: usize, v: u8) {
    (a as *mut u8).write_unaligned(v)
}
pub unsafe fn w16(a: usize, v: u16) {
    (a as *mut u16).write_unaligned(v)
}
pub unsafe fn w32(a: usize, v: u32) {
    (a as *mut u32).write_unaligned(v)
}
pub unsafe fn w64(a: usize, v: u64) {
    (a as *mut u64).write_unaligned(v)
}
pub unsafe fn r32(a: usize) -> u32 {
    (a as *const u32).read_unaligned()
}
pub unsafe fn r64(a: usize) -> u64 {
    (a as *const u64).read_unaligned()
}
pub unsafe fn zero(a: usize, n: usize) {
    std::ptr::write_bytes(a as *mut u8, 0, n)
}
pub unsafe fn astr(a: usize) -> String {
    if a == 0 {
        return String::new();
    }
    std::ffi::CStr::from_ptr(a as *const libc::c_char).to_string_lossy().into_owned()
}
pub unsafe fn wstr(a: usize) -> String {
    if a == 0 {
        return String::new();
    }
    let mut v = Vec::new();
    let mut p = a as *const u16;
    while *p != 0 {
        v.push(*p);
        p = p.add(1);
    }
    String::from_utf16_lossy(&v)
}
pub unsafe fn wlen(a: usize) -> usize {
    let mut n = 0;
    while *((a as *const u16).add(n)) != 0 {
        n += 1;
    }
    n
}
/// Copy `s` + NUL into an ANSI buffer of `cap` bytes; returns length without NUL (or needed size).
pub unsafe fn put_astr(buf: usize, cap: usize, s: &str) -> usize {
    let b = s.as_bytes();
    if buf == 0 || cap <= b.len() {
        return b.len() + 1;
    }
    std::ptr::copy_nonoverlapping(b.as_ptr(), buf as *mut u8, b.len());
    w8(buf + b.len(), 0);
    b.len()
}
pub unsafe fn put_wstr(buf: usize, cap: usize, s: &str) -> usize {
    let v: Vec<u16> = s.encode_utf16().collect();
    if buf == 0 || cap <= v.len() {
        return v.len() + 1;
    }
    std::ptr::copy_nonoverlapping(v.as_ptr(), buf as *mut u16, v.len());
    w16(buf + v.len() * 2, 0);
    v.len()
}

// ---- logging ---------------------------------------------------------------------------------
pub static VERBOSE: AtomicU32 = AtomicU32::new(0);
#[macro_export]
macro_rules! log {
    ($lvl:expr, $($a:tt)*) => {
        if $crate::rt::VERBOSE.load(std::sync::atomic::Ordering::Relaxed) >= $lvl {
            eprintln!("[{}] {}", $crate::rt::cur_tid(), format!($($a)*));
        }
    };
}

// ---- last error ------------------------------------------------------------------------------
thread_local! {
    static LAST_ERROR: Cell<u32> = const { Cell::new(0) };
    static TID: Cell<u32> = const { Cell::new(0) };
    static TLS: RefCell<Vec<usize>> = RefCell::new(vec![0; 1088]);
}
pub fn set_err(e: u32) {
    LAST_ERROR.with(|c| c.set(e))
}
pub fn get_err() -> u32 {
    LAST_ERROR.with(|c| c.get())
}
pub fn cur_tid() -> u32 {
    TID.with(|c| c.get())
}
pub fn tls_get(i: usize) -> usize {
    TLS.with(|t| t.borrow().get(i).copied().unwrap_or(0))
}
pub fn tls_set(i: usize, v: usize) -> bool {
    TLS.with(|t| {
        let mut t = t.borrow_mut();
        if i < t.len() {
            t[i] = v;
            true
        } else {
            false
        }
    })
}

// ---- TEB / PEB (MSVC code reads gs:[0x10] stack limit in __chkstk, gs:[0x30] self, ...) --------
static PEB: OnceLock<usize> = OnceLock::new();
pub const HEAP_HANDLE: usize = 0x0001_0000;

fn peb() -> usize {
    *PEB.get_or_init(|| {
        let p = Box::leak(vec![0u8; 0x1000].into_boxed_slice()).as_mut_ptr() as usize;
        unsafe { w64(p + 0x30, HEAP_HANDLE as u64) };
        p
    })
}

static NEXT_TID: AtomicU32 = AtomicU32::new(0x100);

/// Give the calling OS thread a Windows identity: thread id + TEB in GS.
pub fn init_thread(tid: u32) {
    TID.with(|c| c.set(tid));
    let teb = Box::leak(vec![0u8; 0x2000].into_boxed_slice()).as_mut_ptr() as usize;
    let tls_array = Box::leak(vec![0u8; 64 * 8].into_boxed_slice()).as_mut_ptr() as usize;
    let sp = &teb as *const usize as usize;
    unsafe {
        w64(teb + 0x08, (sp + 0x10000) as u64); // StackBase (approximate)
        w64(teb + 0x10, 0); // StackLimit 0: __chkstk never probes (Linux stacks are mapped)
        w64(teb + 0x30, teb as u64); // Self
        w64(teb + 0x40, libc::getpid() as u64);
        w64(teb + 0x48, tid as u64);
        w64(teb + 0x58, tls_array as u64);
        w64(teb + 0x60, peb() as u64);
        libc::syscall(libc::SYS_arch_prctl, 0x1001 /* ARCH_SET_GS */, teb);
    }
}
pub fn new_tid() -> u32 {
    NEXT_TID.fetch_add(4, Ordering::Relaxed)
}

// ---- objects & handles -----------------------------------------------------------------------
pub struct Event {
    pub manual: bool,
    pub set: AtomicBool,
}
pub struct ThreadObj {
    pub tid: u32,
    pub done: AtomicBool,
    pub code: AtomicU32,
    pub suspended: AtomicBool,
}

pub enum Obj {
    File(i32),
    Comm(Arc<crate::comm::Comm>),
    Nmea,
    Event(Arc<Event>),
    Thread(Arc<ThreadObj>),
    Std(i32),
    Key,
}

static HANDLES: OnceLock<Mutex<HashMap<usize, Arc<Obj>>>> = OnceLock::new();
static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(0x100);
fn handles() -> MutexGuard<'static, HashMap<usize, Arc<Obj>>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap()
}
pub fn new_handle(o: Obj) -> usize {
    let h = NEXT_HANDLE.fetch_add(4, Ordering::Relaxed);
    handles().insert(h, Arc::new(o));
    h
}
pub fn get_handle(h: usize) -> Option<Arc<Obj>> {
    handles().get(&h).cloned()
}
pub fn close_handle(h: usize) -> bool {
    let o = handles().remove(&h);
    match o.as_deref() {
        Some(Obj::File(fd)) => {
            unsafe { libc::close(*fd) };
            true
        }
        Some(_) => true,
        None => false,
    }
}

// ---- sync: one global lock + condvar for all waitable state (events, threads, APCs) ------------
pub struct Sync {
    pub apcs: HashMap<u32, VecDeque<Box<dyn FnOnce() + Send>>>,
}
static SYNC: OnceLock<(Mutex<Sync>, Condvar)> = OnceLock::new();
pub fn sync() -> &'static (Mutex<Sync>, Condvar) {
    SYNC.get_or_init(|| (Mutex::new(Sync { apcs: HashMap::new() }), Condvar::new()))
}
pub fn notify() {
    let (m, cv) = sync();
    drop(m.lock().unwrap());
    cv.notify_all();
}
/// Queue a completion routine to run on thread `tid` during its next alertable wait.
pub fn queue_apc(tid: u32, f: Box<dyn FnOnce() + Send>) {
    let (m, cv) = sync();
    m.lock().unwrap().apcs.entry(tid).or_default().push_back(f);
    cv.notify_all();
}

pub const WAIT_TIMEOUT: u32 = 0x102;
pub const WAIT_IO_COMPLETION: u32 = 0xc0;
pub const WAIT_FAILED: u32 = 0xffff_ffff;
pub const INFINITE: u32 = 0xffff_ffff;

fn signaled(o: &Obj, consume: bool) -> bool {
    match o {
        Obj::Event(e) => {
            let s = e.set.load(Ordering::SeqCst);
            if s && consume && !e.manual {
                e.set.store(false, Ordering::SeqCst);
            }
            s
        }
        Obj::Thread(t) => t.done.load(Ordering::SeqCst),
        _ => true,
    }
}

pub fn wait(hs: &[usize], all: bool, ms: u32, alertable: bool) -> u32 {
    let objs: Vec<Option<Arc<Obj>>> = hs.iter().map(|h| get_handle(*h)).collect();
    if objs.iter().any(|o| o.is_none()) {
        set_err(6);
        return WAIT_FAILED;
    }
    let objs: Vec<Arc<Obj>> = objs.into_iter().map(|o| o.unwrap()).collect();
    let deadline = if ms == INFINITE { None } else { Some(Instant::now() + Duration::from_millis(ms as u64)) };
    let me = cur_tid();
    let (m, cv) = sync();
    let mut g = m.lock().unwrap();
    loop {
        if alertable {
            if let Some(q) = g.apcs.get_mut(&me) {
                if !q.is_empty() {
                    let run: Vec<_> = q.drain(..).collect();
                    drop(g);
                    for f in run {
                        f();
                    }
                    return WAIT_IO_COMPLETION;
                }
            }
        }
        if !objs.is_empty() {
            if all {
                if objs.iter().all(|o| signaled(o, false)) {
                    for o in &objs {
                        signaled(o, true);
                    }
                    return 0;
                }
            } else if let Some(i) = objs.iter().position(|o| signaled(o, true)) {
                return i as u32;
            }
        }
        let now = Instant::now();
        let left = match deadline {
            Some(d) if now >= d => return WAIT_TIMEOUT,
            Some(d) => d - now,
            None => Duration::from_secs(3600),
        };
        g = cv.wait_timeout(g, left).unwrap().0;
    }
}

// ---- threads ----------------------------------------------------------------------------------
pub type ThreadProc = extern "win64" fn(usize) -> u32;

pub fn spawn(start: usize, param: usize, suspended: bool, stack: usize) -> (usize, u32) {
    let tid = new_tid();
    let t = Arc::new(ThreadObj {
        tid,
        done: AtomicBool::new(false),
        code: AtomicU32::new(259),
        suspended: AtomicBool::new(suspended),
    });
    let h = new_handle(Obj::Thread(t.clone()));
    let stack = stack.max(8 << 20);
    std::thread::Builder::new()
        .name(format!("win-{tid:x}"))
        .stack_size(stack)
        .spawn(move || {
            init_thread(tid);
            {
                let (m, cv) = sync();
                let mut g = m.lock().unwrap();
                while t.suspended.load(Ordering::SeqCst) {
                    g = cv.wait(g).unwrap();
                }
            }
            log!(2, "thread start {start:#x}({param:#x})");
            let f: ThreadProc = unsafe { std::mem::transmute(start) };
            let code = f(param);
            finish_thread(&t, code);
        })
        .expect("spawn");
    (h, tid)
}

pub fn finish_thread(t: &ThreadObj, code: u32) {
    t.code.store(code, Ordering::SeqCst);
    t.done.store(true, Ordering::SeqCst);
    notify();
    log!(2, "thread {:x} exit {code}", t.tid);
}

/// Thread object of the calling thread (for ExitThread).
pub fn current_thread_obj() -> Option<Arc<ThreadObj>> {
    let me = cur_tid();
    handles().values().find_map(|o| match &**o {
        Obj::Thread(t) if t.tid == me => Some(t.clone()),
        _ => None,
    })
}
