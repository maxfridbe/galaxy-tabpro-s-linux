"""Usage: RTRE_SYS=/path/to/RTKVHD64.sys python emu.py 0x39ee78   (dumps codec-bridge/coef traffic of a driver function)"""
"""Emulate an RTKVHD64.sys function and log smart-amp bridge / coef traffic."""
import struct, sys
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UC_PROT_ALL
from unicorn.x86_const import *
from rtre import pe, BASE, img, FUNCS

FUNC_STARTS = {f[0] for f in FUNCS}
LOG = []
READ_RESP = {}          # (addr, reg) -> value for bridge reads
STOP = 0xdead0000

def run(entry, rcx_obj_init=None, max_insn=2_000_000, reads=None, extra_run=()):
    if reads: READ_RESP.update(reads)
    mu = Uc(UC_ARCH_X86, UC_MODE_64)
    size = (len(img) + 0xfff) & ~0xfff
    mu.mem_map(BASE, size, UC_PROT_ALL)
    mu.mem_write(BASE, bytes(img))
    # stub page: xor eax,eax ; ret
    STUB = 0x7000_0000
    mu.mem_map(STUB, 0x1000); mu.mem_write(STUB, b'\x31\xc0\xc3')
    mu.mem_map(STOP & ~0xfff, 0x1000); mu.mem_write(STOP, b'\xf4')
    # IAT -> stub
    for entry_imp in pe.DIRECTORY_ENTRY_IMPORT:
        for imp in entry_imp.imports:
            mu.mem_write(imp.address, struct.pack('<Q', STUB))
    # heap objects
    HEAP = 0x2000_0000
    mu.mem_map(HEAP, 0x100000)
    ctx, codec, bridge = HEAP, HEAP + 0x20000, HEAP + 0x40000
    w64 = lambda a, v: mu.mem_write(a, struct.pack('<Q', v))
    w32 = lambda a, v: mu.mem_write(a, struct.pack('<I', v))
    w64(ctx + 0x48, codec); w64(ctx + 8, codec)
    w64(codec + 0x18e0, bridge); w64(bridge + 0x10, codec)
    w32(codec + 0x1c4c, 0x298)
    for off, val in {0x18: 0x22, 0x1c: 0x23, 0x20: 0x24, 0x24: 0x25, 0x30: 0x26, 0x28: 0x27, 0x2c: 0x28, 0x34: 0x89}.items():
        w32(bridge + off, val)
    if rcx_obj_init: rcx_obj_init(mu, ctx, codec, bridge)
    STACK = 0x3000_0000
    mu.mem_map(STACK, 0x100000)
    sp = STACK + 0x80000
    w64(sp, STOP)
    mu.reg_write(UC_X86_REG_RSP, sp)
    mu.reg_write(UC_X86_REG_RCX, ctx)
    count = [0]

    def arg5(): return struct.unpack('<Q', mu.mem_read(mu.reg_read(UC_X86_REG_RSP) + 0x28, 8))[0]
    def ret(v=0):
        rsp = mu.reg_read(UC_X86_REG_RSP)
        ra = struct.unpack('<Q', mu.mem_read(rsp, 8))[0]
        mu.reg_write(UC_X86_REG_RSP, rsp + 8); mu.reg_write(UC_X86_REG_RAX, v); mu.reg_write(UC_X86_REG_RIP, ra)

    def hook(mu, addr, size, _):
        count[0] += 1
        if count[0] > max_insn: mu.emu_stop(); return
        if addr == entry or addr not in FUNC_STARTS and addr not in (0x473a0, 0x47570, 0x26faec, 0x12a8c, 0x45a34, 0x45b4c, 0x464cc): return
        if addr in extra_run: return
        dl = mu.reg_read(UC_X86_REG_RDX) & 0xff
        r8 = mu.reg_read(UC_X86_REG_R8) & 0xffffffff
        r9 = mu.reg_read(UC_X86_REG_R9) & 0xffffffff
        if addr == 0x473a0:
            LOG.append(('W', dl, r8, r9, arg5() & 0xffffffff)); ret(0)
        elif addr == 0x47570:
            v = READ_RESP.get((dl, r9), 0)
            p = arg5(); mu.mem_write(p, struct.pack('<I', v))
            LOG.append(('R', dl, r8, r9, v)); ret(0)
        elif addr == 0x26faec:
            LOG.append(('COEF', mu.reg_read(UC_X86_REG_RDX) & 0xffff, r8, r9, arg5() & 0xffffffff)); ret(0)
        elif addr in (0x45a34, 0x464cc):
            mask = struct.unpack('<Q', mu.mem_read(mu.reg_read(UC_X86_REG_RSP) + 0x28, 8))[0] & 0xff if addr == 0x45a34 else 0
            LOG.append(('W8', dl, r8 & 0xff, r9 & 0xff, mask)); ret(1)
        elif addr == 0x45b4c:
            p = mu.reg_read(UC_X86_REG_R9)
            v = READ_RESP.get((dl, r8 & 0xff), 0)
            try: mu.mem_write(p, bytes([v]))
            except Exception: pass
            LOG.append(('R8', dl, r8 & 0xff, v)); ret(1)
        elif addr == 0x12a8c:
            LOG.append(('SLEEP', mu.reg_read(UC_X86_REG_RCX) & 0xffffffff)); ret(0)
        else:
            LOG.append(('CALL', hex(addr))); ret(0)

    mu.hook_add(UC_HOOK_CODE, hook)
    try:
        mu.emu_start(entry, STOP)
    except Exception as e:
        LOG.append(('EXC', str(e), hex(mu.reg_read(UC_X86_REG_RIP))))
    return LOG, count[0]

def fmt(e):
    if e[0] == 'W': return f'bridge WRITE dev={e[1]:#04x} type={e[2]} reg={e[3]:#06x} val={e[4]:#06x}'
    if e[0] == 'R': return f'bridge READ  dev={e[1]:#04x} type={e[2]} reg={e[3]:#06x} -> {e[4]:#06x}'
    if e[0] == 'COEF': return f'coef  idx={e[1]:#04x} val={e[2]:#06x} mask={e[3]:#010x}'
    if e[0] == 'W8': return f'w8   dev={e[1]:#04x} reg={e[2]:#04x} val={e[3]:#04x}' + (f' keepmask={e[4]:#04x}' if e[4] else '')
    if e[0] == 'R8': return f'r8   dev={e[1]:#04x} reg={e[2]:#04x}'
    if e[0] == 'SLEEP': return f'sleep {e[1]} ms'
    return ' '.join(map(str, e))

if __name__ == '__main__':
    entry = int(sys.argv[1], 0)
    log, n = run(entry)
    print(f'{n} instructions')
    for e in log: print(fmt(e))
