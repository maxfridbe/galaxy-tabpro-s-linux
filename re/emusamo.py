"""Emulate SAMOPanel.sys (Samsung AMOLED panel driver) brightness/init functions with Unicorn and log the
DPCD AUX reads/writes it issues through Intel's IGD interface callback (ctx+0x298, command 0x28).
Set LIVE to a pipe to auxsrv.py to forward the accesses to a real panel. RTRE_SYS=.../SAMOPanel.sys"""
import json, os, struct, sys
os.environ.setdefault('RTRE_SYS', 'SAMOPanel.sys')
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_PROT_ALL
from unicorn.x86_const import *
from rtre import pe, BASE, img

SET_BRIGHTNESS = 0x140003a48
SLEEP_FN = 0x140001f6c
LIVE = None


def run(level, ctx_fields=None, entry=None, regs=None, reads=None, then=(), between=None):
    mu = Uc(UC_ARCH_X86, UC_MODE_64)
    size = (len(img) + 0xfff) & ~0xfff
    mu.mem_map(BASE, size, UC_PROT_ALL); mu.mem_write(BASE, bytes(img))
    mu.mem_map(0, 0x1000); mu.mem_write(0, b'\x31\xc0\xc3')
    STUB = 0x7000_0000; mu.mem_map(STUB, 0x10000)
    for i, (e, imp) in enumerate((e, imp) for e in pe.DIRECTORY_ENTRY_IMPORT for imp in e.imports):
        a = STUB + i * 16; mu.mem_write(a, b'\x31\xc0\xc3'); mu.mem_write(imp.address, struct.pack('<Q', a))
    IFACE = 0x7100_0000; mu.mem_map(IFACE, 0x1000); mu.mem_write(IFACE, b'\xc3')
    HEAP = 0x2000_0000; mu.mem_map(HEAP, 0x100000)
    ctx = HEAP
    fields = {0xbfa: 1, 0x2ad: 1}
    fields.update(ctx_fields or {})
    for off, v in fields.items():
        mu.mem_write(ctx + off, bytes([v]))
    mu.mem_write(ctx + 0x298, struct.pack('<Q', IFACE))
    out = HEAP + 0x80000
    STACK = 0x3000_0000; mu.mem_map(STACK, 0x100000); sp = STACK + 0x80000
    STOP = 0xdead0000; mu.mem_map(STOP, 0x1000); mu.mem_write(STOP, b'\xf4')
    mu.mem_write(sp, struct.pack('<Q', STOP))
    mu.reg_write(UC_X86_REG_RSP, sp)
    mu.reg_write(UC_X86_REG_RCX, ctx); mu.reg_write(UC_X86_REG_RDX, level); mu.reg_write(UC_X86_REG_R8, out)
    for r, v in (regs or {}).items(): mu.reg_write(r, v)
    log = []

    def ret(v=0):
        rsp = mu.reg_read(UC_X86_REG_RSP); ra = struct.unpack('<Q', mu.mem_read(rsp, 8))[0]
        mu.reg_write(UC_X86_REG_RSP, rsp + 8); mu.reg_write(UC_X86_REG_RAX, v); mu.reg_write(UC_X86_REG_RIP, ra)

    def hook(mu, addr, size, _):
        if addr == IFACE:
            cmd = mu.reg_read(UC_X86_REG_RCX) & 0xffffffff
            req = mu.reg_read(UC_X86_REG_RDX); st = mu.reg_read(UC_X86_REG_R8)
            dirn = struct.unpack('<I', mu.mem_read(req, 4))[0]
            item = struct.unpack('<Q', mu.mem_read(req + 8, 8))[0]
            dpcd = struct.unpack('<I', mu.mem_read(item, 4))[0]
            dptr = struct.unpack('<Q', mu.mem_read(item + 8, 8))[0]
            ln = struct.unpack('<I', mu.mem_read(item + 16, 4))[0]
            if dirn == 0 and LIVE:
                LIVE.stdin.write(f'R {dpcd} {ln}\n'); LIVE.stdin.flush()
                val = bytes.fromhex(LIVE.stdout.readline().strip()); mu.mem_write(dptr, val); log.append(('R', cmd, dpcd, val.hex()))
            elif dirn == 0 and isinstance(reads, list):
                val = bytes.fromhex(reads.pop(0)) if reads else b'\0' * ln
                mu.mem_write(dptr, val); log.append(('R', cmd, dpcd, val.hex()))
            elif dirn == 0:
                val = bytes((reads or {}).get(dpcd, b'\0' * ln))[:ln].ljust(ln, b'\0')
                mu.mem_write(dptr, val); log.append(('R', cmd, dpcd, val.hex()))
            else:
                data = bytes(mu.mem_read(dptr, max(ln, 1)))
                log.append(('W', cmd, dpcd, data.hex()))
                if LIVE:
                    LIVE.stdin.write(f'W {dpcd} {data.hex()}\n'); LIVE.stdin.flush()
                    a = LIVE.stdout.readline().strip()
                    if a != 'ok': log.append(('ERR', a))
            mu.mem_write(st, b'\0\0\0\0'); ret(0)
        elif addr == 0x140003aa0:   # movabs rax,[KUSER_SHARED_DATA+0x320] (tick count)
            mu.reg_write(UC_X86_REG_RAX, 0); mu.reg_write(UC_X86_REG_RIP, addr + 10)
        elif addr == SLEEP_FN:
            us = mu.reg_read(UC_X86_REG_RCX) & 0xffffffff
            log.append(('SLEEP_US', us)); ret(0)
            if LIVE: import time; time.sleep(us / 1e6)

    mu.hook_add(UC_HOOK_CODE, hook)
    try:
        mu.emu_start(entry or SET_BRIGHTNESS, STOP, count=2_000_000)
    except Exception as e:
        log.append(('EXC', str(e), hex(mu.reg_read(UC_X86_REG_RIP))))
    for ent, rdx, r8 in then:
        if between: between(ent, rdx)
        mu.mem_write(sp, struct.pack('<Q', STOP)); mu.reg_write(UC_X86_REG_RSP, sp)
        mu.reg_write(UC_X86_REG_RCX, ctx); mu.reg_write(UC_X86_REG_RDX, rdx); mu.reg_write(UC_X86_REG_R8, r8)
        log.append(('CALL', hex(ent), rdx))
        try:
            mu.emu_start(ent, STOP, count=2_000_000)
        except Exception as e:
            log.append(('EXC', str(e), hex(mu.reg_read(UC_X86_REG_RIP))))
    log.append(('CTX', bytes(mu.mem_read(ctx, 0xd00)).hex()))
    return log


if __name__ == '__main__':
    levels = [int(a) for a in sys.argv[1:]] or list(range(0, 101))
    res = {}
    for lv in levels:
        res[lv] = run(lv)
    json.dump(res, open('samo-brightness.json', 'w'), indent=0)
    for lv in levels[:3] + levels[-2:]:
        print(f'--- level {lv}: {len(res[lv])} ops')
        for e in res[lv]: print('  ', e)
