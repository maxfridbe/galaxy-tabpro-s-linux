//! Minimal PE32+ loader: map sections, apply base relocations, bind imports, read exports.

use std::collections::HashMap;

pub struct Image {
    pub base: usize,
    pub size: usize,
    pub entry: usize,
    exports: HashMap<String, usize>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

unsafe fn rd32(a: usize) -> u32 {
    (a as *const u32).read_unaligned()
}
unsafe fn rd16(a: usize) -> u16 {
    (a as *const u16).read_unaligned()
}
unsafe fn cstr(a: usize) -> String {
    std::ffi::CStr::from_ptr(a as *const libc::c_char).to_string_lossy().into_owned()
}

impl Image {
    pub fn base_range(&self) -> std::ops::Range<usize> {
        self.base..self.base + self.size
    }

    pub fn export(&self, name: &str) -> Option<usize> {
        self.exports.get(name).copied()
    }

    /// Load `file`, resolving imports through `resolve(dll, name) -> address`.
    pub fn load(file: &[u8], resolve: &mut dyn FnMut(&str, &str) -> usize) -> Result<Image, String> {
        if file.len() < 0x40 || &file[0..2] != b"MZ" {
            return Err("not a PE file".into());
        }
        let nt = u32_at(file, 0x3c) as usize;
        if &file[nt..nt + 4] != b"PE\0\0" || u16_at(file, nt + 4) != 0x8664 {
            return Err("not an x86-64 PE image".into());
        }
        let nsec = u16_at(file, nt + 6) as usize;
        let opt = nt + 24;
        let opt_size = u16_at(file, nt + 20) as usize;
        if u16_at(file, opt) != 0x20b {
            return Err("not PE32+".into());
        }
        let entry_rva = u32_at(file, opt + 16) as usize;
        let pref_base = u64_at(file, opt + 24) as usize;
        let size = u32_at(file, opt + 56) as usize;
        let hdr_size = u32_at(file, opt + 60) as usize;
        let dd = |i: usize| (u32_at(file, opt + 112 + i * 8) as usize, u32_at(file, opt + 116 + i * 8) as usize);

        // map at the preferred base if possible (fewer relocations, addresses match disassembly)
        let prot = libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC;
        let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS;
        let mut base = unsafe {
            libc::mmap(pref_base as *mut _, size, prot, flags | libc::MAP_FIXED_NOREPLACE, -1, 0)
        } as usize;
        if base == libc::MAP_FAILED as usize || base != pref_base {
            base = unsafe { libc::mmap(std::ptr::null_mut(), size, prot, flags, -1, 0) } as usize;
            if base == libc::MAP_FAILED as usize {
                return Err("mmap failed".into());
            }
        }
        let mem = unsafe { std::slice::from_raw_parts_mut(base as *mut u8, size) };
        mem[..hdr_size].copy_from_slice(&file[..hdr_size]);
        let sec0 = opt + opt_size;
        for i in 0..nsec {
            let s = sec0 + i * 40;
            let va = u32_at(file, s + 12) as usize;
            let raw_size = u32_at(file, s + 16) as usize;
            let raw_ptr = u32_at(file, s + 20) as usize;
            let n = raw_size.min(size - va).min(file.len().saturating_sub(raw_ptr));
            mem[va..va + n].copy_from_slice(&file[raw_ptr..raw_ptr + n]);
        }

        // base relocations
        let delta = base.wrapping_sub(pref_base);
        if delta != 0 {
            let (rva, rsize) = dd(5);
            let mut o = rva;
            while o < rva + rsize {
                let page = unsafe { rd32(base + o) } as usize;
                let bsize = unsafe { rd32(base + o + 4) } as usize;
                if bsize < 8 {
                    break;
                }
                for k in 0..(bsize - 8) / 2 {
                    let e = unsafe { rd16(base + o + 8 + k * 2) };
                    match e >> 12 {
                        0 => {}
                        10 => unsafe {
                            let p = (base + page + (e & 0xfff) as usize) as *mut usize;
                            p.write_unaligned(p.read_unaligned().wrapping_add(delta));
                        },
                        t => return Err(format!("unsupported relocation type {t}")),
                    }
                }
                o += bsize;
            }
        }

        // imports
        let (irva, _) = dd(1);
        if irva != 0 {
            let mut d = base + irva;
            loop {
                let (oft, name_rva, ft) = unsafe { (rd32(d) as usize, rd32(d + 12) as usize, rd32(d + 16) as usize) };
                if name_rva == 0 {
                    break;
                }
                let dll = unsafe { cstr(base + name_rva) };
                let lookup = if oft != 0 { oft } else { ft };
                let mut k = 0;
                loop {
                    let ent = unsafe { ((base + lookup + k * 8) as *const u64).read_unaligned() };
                    if ent == 0 {
                        break;
                    }
                    let name = if ent >> 63 != 0 {
                        format!("#{}", ent & 0xffff)
                    } else {
                        unsafe { cstr(base + ent as usize + 2) }
                    };
                    let addr = resolve(&dll, &name);
                    unsafe { ((base + ft + k * 8) as *mut usize).write_unaligned(addr) };
                    k += 1;
                }
                d += 20;
            }
        }

        // exports
        let mut exports = HashMap::new();
        let (erva, _) = dd(0);
        if erva != 0 {
            let e = base + erva;
            unsafe {
                let nnames = rd32(e + 24) as usize;
                let funcs = base + rd32(e + 28) as usize;
                let names = base + rd32(e + 32) as usize;
                let ords = base + rd32(e + 36) as usize;
                for i in 0..nnames {
                    let n = cstr(base + rd32(names + i * 4) as usize);
                    let o = rd16(ords + i * 2) as usize;
                    exports.insert(n, base + rd32(funcs + o * 4) as usize);
                }
            }
        }
        Ok(Image { base, size, entry: if entry_rva != 0 { base + entry_rva } else { 0 }, exports })
    }
}
