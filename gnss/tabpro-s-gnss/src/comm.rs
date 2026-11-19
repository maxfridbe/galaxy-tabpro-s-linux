//! Windows serial-port semantics on top of /dev/gnssN (gnss-bcm4752 driver).
//! A reader thread fills an RX buffer; reads follow COMMTIMEOUTS; the baud rate set through
//! SetCommState goes to the driver's `speed` attribute.

use crate::log;
use crate::rt::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub struct Paths {
    pub dev: String,
    pub speed: String,
    pub regpu: String,
}

#[derive(Clone, Copy, Default)]
pub struct Timeouts {
    pub ri: u32,
    pub rm: u32,
    pub rc: u32,
    pub wm: u32,
    pub wc: u32,
}

pub struct Comm {
    fd: i32,
    rx: Mutex<VecDeque<u8>>,
    rx_cv: Condvar,
    pub timeouts: Mutex<Timeouts>,
    pub baud: AtomicU32,
    cancel_gen: AtomicU64,
    speed_path: String,
    pub stats: [AtomicU64; 2],
    /// in-order workers for asynchronous reads (0) and writes (1), like a Windows serial driver
    jobs: [Mutex<std::sync::mpsc::Sender<Box<dyn FnOnce() + Send>>>; 2],
}

const MAXDWORD: u32 = 0xffff_ffff;

impl Comm {
    pub fn open(p: &Paths) -> Result<Arc<Comm>, i32> {
        let c = std::ffi::CString::new(p.dev.clone()).unwrap();
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(5));
        }
        let mk = |name: &str| {
            let (tx, rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
            std::thread::Builder::new().name(name.into()).spawn(move || {
                for job in rx {
                    job();
                }
            }).unwrap();
            Mutex::new(tx)
        };
        let comm = Arc::new(Comm {
            jobs: [mk("comm-read-q"), mk("comm-write-q")],
            fd,
            rx: Mutex::new(VecDeque::new()),
            rx_cv: Condvar::new(),
            timeouts: Mutex::new(Timeouts::default()),
            baud: AtomicU32::new(std::fs::read_to_string(&p.speed).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(115200)),
            cancel_gen: AtomicU64::new(0),
            speed_path: p.speed.clone(),
            stats: [AtomicU64::new(0), AtomicU64::new(0)],
        });
        let c2 = comm.clone();
        std::thread::Builder::new()
            .name("comm-rx".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    let n = unsafe { libc::read(c2.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
                    if n <= 0 {
                        if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                            continue;
                        }
                        log!(0, "comm: read ended ({n})");
                        return;
                    }
                    c2.stats[1].fetch_add(n as u64, Ordering::Relaxed);
                    log!(3, "comm rx {}", hex(&buf[..n as usize]));
                    c2.rx.lock().unwrap().extend(&buf[..n as usize]);
                    c2.rx_cv.notify_all();
                    notify();
                }
            })
            .unwrap();
        log!(1, "comm: opened {} at {} baud", p.dev, comm.baud.load(Ordering::Relaxed));
        Ok(comm)
    }

    pub fn available(&self) -> usize {
        self.rx.lock().unwrap().len()
    }

    pub fn purge_rx(&self) {
        self.rx.lock().unwrap().clear();
    }

    pub fn submit(&self, write: bool, job: Box<dyn FnOnce() + Send>) {
        let _ = self.jobs[write as usize].lock().unwrap().send(job);
    }

    pub fn generation(&self) -> u64 {
        self.cancel_gen.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel_gen.fetch_add(1, Ordering::SeqCst);
        self.rx_cv.notify_all();
    }

    pub fn set_baud(&self, b: u32) {
        if b != 0 && self.baud.swap(b, Ordering::SeqCst) != b {
            log!(1, "comm: baud -> {b}");
            if let Err(e) = std::fs::write(&self.speed_path, b.to_string()) {
                log!(0, "comm: cannot set speed: {e}");
            }
        }
    }

    /// Read with Windows COMMTIMEOUTS semantics. Returns Err(995) if cancelled.
    pub fn read(&self, out: &mut [u8]) -> Result<usize, u32> {
        self.read_gen(out, self.generation())
    }

    /// `gen`: cancel generation when the request was submitted (CancelIo/PurgeComm abort it).
    pub fn read_gen(&self, out: &mut [u8], gen: u64) -> Result<usize, u32> {
        let t = *self.timeouts.lock().unwrap();
        if self.cancel_gen.load(Ordering::SeqCst) != gen {
            return Err(995);
        }
        let n = out.len();
        let start = Instant::now();
        // total timeout (None = infinite)
        let (total, interval, return_any) = if t.ri == MAXDWORD && t.rm == 0 && t.rc == 0 {
            (Some(Duration::ZERO), None, true)
        } else if t.ri == MAXDWORD && t.rm == MAXDWORD && t.rc > 0 && t.rc < MAXDWORD {
            (Some(Duration::from_millis(t.rc as u64)), None, true)
        } else {
            let tot = t.rm as u64 * n as u64 + t.rc as u64;
            (if tot == 0 { None } else { Some(Duration::from_millis(tot)) },
             if t.ri == 0 { None } else { Some(Duration::from_millis(t.ri as u64)) }, false)
        };
        let mut got = 0usize;
        let mut last_byte = Instant::now();
        let mut rx = self.rx.lock().unwrap();
        loop {
            while got < n {
                match rx.pop_front() {
                    Some(b) => {
                        out[got] = b;
                        got += 1;
                        last_byte = Instant::now();
                    }
                    None => break,
                }
            }
            if got == n || (return_any && got > 0) {
                return Ok(got);
            }
            if self.cancel_gen.load(Ordering::SeqCst) != gen {
                return Err(995);
            }
            let now = Instant::now();
            let mut wait = Duration::from_millis(50);
            if let Some(tot) = total {
                let end = start + tot;
                if now >= end {
                    return Ok(got);
                }
                wait = wait.min(end - now);
            }
            if let (Some(iv), true) = (interval, got > 0) {
                let end = last_byte + iv;
                if now >= end {
                    return Ok(got);
                }
                wait = wait.min(end - now);
            }
            rx = self.rx_cv.wait_timeout(rx, wait).unwrap().0;
        }
    }

    pub fn write(&self, data: &[u8]) -> Result<usize, u32> {
        log!(3, "comm tx {}", hex(data));
        let mut off = 0;
        while off < data.len() {
            let n = unsafe { libc::write(self.fd, data[off..].as_ptr() as *const _, data.len() - off) };
            if n < 0 {
                let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                if e == libc::EINTR || e == libc::EAGAIN {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                log!(0, "comm: write error {e}");
                return Err(31);
            }
            off += n as usize;
        }
        self.stats[0].fetch_add(data.len() as u64, Ordering::Relaxed);
        Ok(data.len())
    }
}

pub fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for (i, x) in b.iter().enumerate().take(64) {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{x:02x}"));
    }
    if b.len() > 64 {
        s.push_str(&format!(" … ({} bytes)", b.len()));
    }
    s
}
