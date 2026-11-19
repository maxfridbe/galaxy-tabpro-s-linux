"""Emulate SkcController.sys Samsung TPS68470 power-on to recover PMIC register writes."""
import os, struct, sys
os.environ.setdefault('RTRE_SYS', 'SkcController.sys')   # or: RTRE_SYS=/path/to/SkcController.sys
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_PROT_ALL
from unicorn.x86_const import *
from rtre import pe, BASE, img, FUNCS

I2C_WRITE = 0x14000f978
I2C_READ = 0x14000f848
REGN = {0x0a:'XTALDIV',0x0b:'PLLDIV',0x0c:'POSTDIV',0x06:'POSTDIV2',0x07:'BOOSTDIV',0x08:'BUCKDIV',0x09:'PLLSWR',0x0d:'PLLCTL',0x0e:'PLLCTL2',0x0f:'CLKCFG1',0x10:'CLKCFG2',
        0x14:'GPCTL0A',0x15:'GPCTL0B',0x16:'GPCTL1A',0x17:'GPCTL1B',0x18:'GPCTL2A',0x19:'GPCTL2B',0x1a:'GPCTL3A',0x1b:'GPCTL3B',0x1c:'GPCTL4A',0x1d:'GPCTL4B',
        0x1e:'GPCTL5A',0x1f:'GPCTL5B',0x20:'GPCTL6A',0x21:'GPCTL6B',0x22:'SGPO',0x26:'GPDI',0x27:'GPDO',0x3c:'VCMVAL',0x3d:'VAUX1VAL',0x3e:'VAUX2VAL',
        0x3f:'VIOVAL',0x40:'VSIOVAL',0x41:'VAVAL',0x42:'VDVAL',0x43:'S_I2C_CTL',0x44:'VCMCTL',0x45:'VAUX1CTL',0x46:'VAUX2CTL',0x47:'VACTL',0x48:'VDCTL',0x50:'RESET',0xff:'REVID'}

class Emu:
    def __init__(self):
        self.mu = mu = Uc(UC_ARCH_X86, UC_MODE_64)
        size = (len(img) + 0xfff) & ~0xfff
        mu.mem_map(BASE, size, UC_PROT_ALL); mu.mem_write(BASE, bytes(img))
        self.STUB = 0x7000_0000; mu.mem_map(self.STUB, 0x10000)
        self.imp = {}
        for i, (e, imp) in enumerate((e, imp) for e in pe.DIRECTORY_ENTRY_IMPORT for imp in e.imports):
            a = self.STUB + i * 16
            mu.mem_write(a, b'\xc3'); mu.mem_write(imp.address, struct.pack('<Q', a))
            self.imp[a] = (imp.name or b'?').decode()
        self.HEAP = 0x2000_0000; mu.mem_map(self.HEAP, 0x400000); self.hp = self.HEAP + 0x1000
        self.STACK = 0x3000_0000; mu.mem_map(self.STACK, 0x100000)
        mu.mem_map(0, 0x1000); mu.mem_write(0, b'\x31\xc0\xc3')   # null fn ptrs (WDF table) -> return 0
        self.STOP = 0xdead0000; mu.mem_map(self.STOP, 0x1000); mu.mem_write(self.STOP, b'\xf4')
        self.log = []; self.regs = {}
        mu.hook_add(UC_HOOK_CODE, self.hook)
    def alloc(self, n):
        a = self.hp; self.hp += (n + 0xf) & ~0xf; return a
    def r(self, reg): return self.mu.reg_read(reg)
    def ret(self, v=0):
        rsp = self.r(UC_X86_REG_RSP); ra = struct.unpack('<Q', self.mu.mem_read(rsp, 8))[0]
        self.mu.reg_write(UC_X86_REG_RSP, rsp + 8); self.mu.reg_write(UC_X86_REG_RAX, v); self.mu.reg_write(UC_X86_REG_RIP, ra)
    def hook(self, mu, addr, size, _):
        if addr in self.imp:
            n = self.imp[addr]
            if n.startswith('ExAllocatePool'):
                sz = self.r(UC_X86_REG_RDX) if 'WithTag' in n or n == 'ExAllocatePool' else self.r(UC_X86_REG_RDX)
                self.ret(self.alloc(max(sz & 0xffffff, 16)))
            elif n in ('KeDelayExecutionThread',):
                p = self.r(UC_X86_REG_R8); t = struct.unpack('<q', mu.mem_read(p, 8))[0]
                self.log.append(('SLEEP', -t // 10000)); self.ret(0)
            elif n in ('KeStallExecutionProcessor',):
                self.log.append(('STALL', self.r(UC_X86_REG_RCX))); self.ret(0)
            else:
                self.ret(0)
            return
        if addr == I2C_WRITE:
            reg = self.r(UC_X86_REG_RDX) & 0xffff; val = self.r(UC_X86_REG_R8) & 0xffffffff; m = self.r(UC_X86_REG_R9) & 0xffffffff
            self.log.append(('W', reg, val, 0)); self.regs[reg] = val & 0xff; self.ret(0)
        elif addr == I2C_READ:
            reg = self.r(UC_X86_REG_RDX) & 0xffff; out = self.r(UC_X86_REG_R8)
            v = self.regs.get(reg, 0x21 if reg == 0xff else 0)
            self.mu.mem_write(out, struct.pack('<I', v)); self.log.append(('R', reg, v)); self.ret(0)
    def call(self, fn, *args, max_insn=3_000_000):
        mu = self.mu; sp = self.STACK + 0x80000
        mu.mem_write(sp, struct.pack('<Q', self.STOP))
        for i, a in enumerate(args[4:]): mu.mem_write(sp + 0x28 + 8 * i, struct.pack('<Q', a))
        for reg, a in zip((UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9), args): mu.reg_write(reg, a)
        mu.reg_write(UC_X86_REG_RSP, sp)
        try:
            mu.emu_start(fn, self.STOP, count=max_insn)
        except Exception as e:
            self.log.append(('EXC', str(e), hex(self.r(UC_X86_REG_RIP))))
        return self.r(UC_X86_REG_RAX)

def fmt(e):
    if e[0] == 'W':
        n = REGN.get(e[1], '?')
        return f"  write {n:9} (0x{e[1]:02x}) = 0x{e[2]:02x}" + (f"  mask=0x{e[3]:x}" if e[3] else '')
    if e[0] == 'R': return f"  read  {REGN.get(e[1],'?'):9} (0x{e[1]:02x}) -> 0x{e[2]:02x}"
    return '  ' + ' '.join(str(x) for x in e)

if __name__ == '__main__':
    for link in (0, 1):
        em = Emu()
        i2c = em.alloc(0x400)               # fake I2C target object
        cfg = em.alloc(0x100)               # ctor config: [+4] copied to this+0x20
        mem = em.alloc(0xc0)
        em.call(0x1400058e8, mem, i2c, cfg)
        sensor = struct.unpack('<Q', em.mu.mem_read(mem + 0x18, 8))[0]
        conf = em.alloc(0x40); c = bytearray(0x40)
        struct.pack_into('<I', c, 0, 5 if link == 0 else 4)
        struct.pack_into('<5H', c, 8, 1200, 2800, 1800, 2800 if link == 0 else 0, 1800)   # from imx241.sys
        em.mu.mem_write(conf, bytes(c))
        rc0 = em.call(0x140008778, sensor, 0, conf)
        em.log.append(('--- SetSensorConf rc', hex(rc0), 'then SensorOn link', link))
        scfg = em.alloc(0x100)
        struct.pack_into('<II', b := bytearray(0x40), 0, 5 if link == 0 else 4, link); em.mu.mem_write(scfg, bytes(b))
        rc = em.call(0x14000663c, mem, link, conf)
        print(f"===== link {link}: SensorOn rc={rc:#x}")
        for e in em.log: print(fmt(e))
