//! tabpro-s-gnss: run Broadcom's GNSS engine (BcmGnss.dll from the Galaxy TabPro S Windows driver)
//! natively on Linux. Loads the DLL with a small PE loader, provides the Windows API it uses, talks
//! to the BCM4752 through /dev/gnssN (gnss-bcm4752 kernel driver) and serves NMEA on a unix socket.
//!
//! Usage:
//!   tabpro-s-gnss [--root DIR] [--dll FILE] [--config FILE] [--socket PATH] [--device DEV] [-v...]
//!   tabpro-s-gnss --make-config BcmGnssConfig.xml [--root DIR]     # write DIR/gpsconfig.xml
//! Lines on stdin are posted to the engine as IRM commands ("$pglirm,...").

mod comm;
mod k32;
mod modules;
mod nmea;
mod other;
mod pe;
mod rt;

use std::sync::OnceLock;

pub static IMAGE: OnceLock<pe::Image> = OnceLock::new();

/// Let the DLL's C runtime flush its files (engine logs, NV storage) before we exit.
pub fn shutdown_engine() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Some(img) = IMAGE.get() {
            if img.entry != 0 {
                let dm: DllMain = unsafe { std::mem::transmute(img.entry) };
                dm(img.base, 0, 1);
            }
        }
    });
}
static CONFIG_PATH: OnceLock<std::ffi::CString> = OnceLock::new();

type GlgpsMain = extern "win64" fn(i32, usize, *const libc::c_char, i32) -> i32;
type PostIrm = extern "win64" fn(*const libc::c_char);
type DllMain = extern "win64" fn(usize, u32, usize) -> i32;

type SetFn = extern "win64" fn(usize);

/// Engine diagnostics: trace(level, message)
extern "win64" fn engine_trace(level: u32, msg: *const libc::c_char) {
    if msg.is_null() {
        return;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(msg) }.to_string_lossy();
    let s = s.trim_end();
    if !s.is_empty() {
        log!(if level <= 2 { 1 } else { 2 }, "gll[{level}] {s}");
    }
}

extern "win64" fn engine_pos_req_state(state: u32) {
    log!(1, "position request state -> {state}");
}

/// Location report (what BcmGnssLocationSensor.dll receives on Windows).
extern "win64" fn engine_location(buf: usize, size: u32, ctx: usize) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 3 || rt::VERBOSE.load(std::sync::atomic::Ordering::Relaxed) >= 2 {
        let b = unsafe { std::slice::from_raw_parts(buf as *const u8, (size as usize).min(0x980)) };
        log!(0, "location report #{n} ({size} B, ctx {ctx:#x}):");
        for (i, c) in b.chunks(32).enumerate().take(24) {
            log!(0, "  {:04x}: {}", i * 32, comm::hex(c));
        }
    }
    nmea::location_report(buf, size as usize);
}

const START_CMD: &str = "$pglirm,req_pos,win_xp_immediate_periodic_request,acc,50";

extern "win64" fn engine_thread(_p: usize) -> u32 {
    let main: GlgpsMain = unsafe { std::mem::transmute(IMAGE.get().unwrap().export("glgps_main").unwrap()) };
    let r = main(0, 0, CONFIG_PATH.get().unwrap().as_ptr(), 0);
    eprintln!("tabpro-s-gnss: engine exited ({r})");
    std::process::exit(if r == 0 { 3 } else { r });
}

fn usage() -> ! {
    eprintln!("usage: tabpro-s-gnss [--root DIR] [--dll FILE] [--config FILE] [--socket PATH] [--device /dev/gnss0] [-v]");
    eprintln!("       tabpro-s-gnss --make-config BcmGnssConfig.xml [--root DIR]");
    std::process::exit(2)
}

/// Windows' BcmGnssConfig.xml with the <hal .../> element replaced for Linux.
fn make_config(src: &str, root: &str) -> Result<String, String> {
    let s = std::fs::read_to_string(src).map_err(|e| format!("{src}: {e}"))?;
    let s = s.trim_start_matches('\u{feff}');
    let a = s.find("<hal").ok_or("no <hal> element")?;
    let b = a + s[a..].find("/>").ok_or("unterminated <hal>")? + 2;
    let hal = r#"<hal
    PortName="BCMGNSS1"
    BaudRate="921600"
    NmeaOutName="NMEAOUT"
    DisableCoreDumpUponAssert="-1"
    LogEnabled="false"
    DisableConsoleIO="true"
    LogDirectory="C:\log"
    AssistanceSources="0x0"
    NvStoWriteToFile="true"
    LtoDownloadIntervalSeconds="14400"
    LogLineThreshold="1000000"
/>"#;
    let _ = root;
    Ok(format!("{}{}{}", &s[..a], hal, &s[b..]))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut root = "/var/lib/tabpro-s-gnss".to_string();
    let (mut dll, mut config, mut socket) = (None, None, "/run/tabpro-s-gnss/nmea.sock".to_string());
    let mut device = None;
    let mut make = None;
    let mut cmds: Vec<String> = vec![];
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().unwrap_or_else(|| usage()),
            "--dll" => dll = Some(args.next().unwrap_or_else(|| usage())),
            "--config" => config = Some(args.next().unwrap_or_else(|| usage())),
            "--socket" => socket = args.next().unwrap_or_else(|| usage()),
            "--device" => device = Some(args.next().unwrap_or_else(|| usage())),
            "--make-config" => make = Some(args.next().unwrap_or_else(|| usage())),
            "--irm" => cmds.push(args.next().unwrap_or_else(|| usage())),
            "-v" => { rt::VERBOSE.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
            "-vv" => { rt::VERBOSE.fetch_add(2, std::sync::atomic::Ordering::Relaxed); }
            "-vvv" => { rt::VERBOSE.fetch_add(3, std::sync::atomic::Ordering::Relaxed); }
            _ => usage(),
        }
    }
    if let Some(src) = make {
        match make_config(&src, &root) {
            Ok(x) => {
                let out = format!("{root}/gpsconfig.xml");
                std::fs::create_dir_all(&root).ok();
                std::fs::write(&out, x).unwrap_or_else(|e| { eprintln!("{out}: {e}"); std::process::exit(1) });
                println!("wrote {out}");
                return;
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1)
            }
        }
    }
    let dll = dll.unwrap_or(format!("{root}/BcmGnss.dll"));
    let config = config.unwrap_or("gpsconfig.xml".into()); // relative -> root

    // the gnss-bcm4752 device and its control attributes
    let device = device.unwrap_or_else(|| {
        std::fs::read_dir("/sys/class/gnss").ok().and_then(|d| {
            d.flatten().find(|e| e.path().join("device/regpu").exists()).map(|e| format!("/dev/{}", e.file_name().to_string_lossy()))
        }).unwrap_or_else(|| "/dev/gnss0".into())
    });
    let sysdev = std::env::var("TABPRO_GNSS_SYSFS")
        .unwrap_or_else(|_| format!("/sys/class/gnss/{}/device", device.rsplit('/').next().unwrap_or("gnss0")));
    let file = std::fs::read(&dll).unwrap_or_else(|e| {
        eprintln!("tabpro-s-gnss: cannot read {dll}: {e} (copy BcmGnss.dll from the Windows driver, see docs)");
        std::process::exit(1)
    });

    rt::init_thread(rt::new_tid());
    let image = pe::Image::load(&file, &mut modules::resolve_import).unwrap_or_else(|e| {
        eprintln!("tabpro-s-gnss: {dll}: {e}");
        std::process::exit(1)
    });
    let missing = modules::missing();
    if !missing.is_empty() {
        crate::log!(1, "imports without implementation (abort if called): {}", missing.join(", "));
    }
    k32::CONFIG
        .set(k32::Config {
            root: root.clone(),
            chip_ports: vec!["BCMGNSS1".into()],
            nmea_port: "NMEAOUT".into(),
            comm: comm::Paths { dev: device.clone(), speed: format!("{sysdev}/speed"), regpu: format!("{sysdev}/regpu") },
            image: image.base_range(),
        })
        .ok();
    log!(0, "loaded {dll} at {:#x} ({} KB), device {device}", image.base, image.size / 1024);
    let entry = image.entry;
    let has_main = image.export("glgps_main").is_some();
    let post = image.export("GpsHalPlatfPostIrmCommand");
    IMAGE.set(image).ok().unwrap();
    if !has_main || post.is_none() {
        eprintln!("tabpro-s-gnss: {dll} is not Broadcom's GNSS engine (no glgps_main)");
        std::process::exit(1);
    }
    std::fs::create_dir_all(&root).ok();
    std::env::set_current_dir(&root).ok();
    if !std::path::Path::new(&k32::to_unix(&config)).exists() {
        eprintln!("tabpro-s-gnss: no {} — generate it with --make-config BcmGnssConfig.xml", k32::to_unix(&config));
        std::process::exit(1);
    }
    CONFIG_PATH.set(std::ffi::CString::new(config).unwrap()).ok();

    if entry != 0 {
        let dm: DllMain = unsafe { std::mem::transmute(entry) };
        let r = dm(IMAGE.get().unwrap().base, 1, 0);
        log!(1, "DllMain(PROCESS_ATTACH) = {r}");
        if r == 0 {
            eprintln!("tabpro-s-gnss: DllMain failed");
            std::process::exit(1);
        }
    }
    nmea::serve(&socket);
    for (name, f) in [("GpsHalSetDriverTraceFuncPtr", engine_trace as usize), ("GpsHalSetLocSensorFuncPtr", engine_location as usize),
                    ("GpsHalSetUpdatePosReqStateFuncPtr", engine_pos_req_state as usize)] {
        if let Some(a) = IMAGE.get().unwrap().export(name) {
            let set: SetFn = unsafe { std::mem::transmute(a) };
            set(f);
        }
    }
    rt::spawn(engine_thread as usize, 0, false, 16 << 20);

    let post: PostIrm = unsafe { std::mem::transmute(post.unwrap()) };
    std::thread::sleep(std::time::Duration::from_secs(3));
    if cmds.is_empty() {
        cmds.push(START_CMD.into());
    }
    for c in &cmds {
        log!(0, "IRM: {c}");
        let cs = std::ffi::CString::new(c.as_str()).unwrap();
        post(cs.as_ptr());
    }
    let mut line = String::new();
    while std::io::stdin().read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
        let c = line.trim();
        if !c.is_empty() {
            log!(0, "IRM: {c}");
            let cs = std::ffi::CString::new(c).unwrap();
            post(cs.as_ptr());
        }
        line.clear();
    }
    loop {
        std::thread::park();
    }
}
