"""Tiny RE helper for Windows x64 drivers (used on RTKVHD64.sys, SkcController.sys, imx241.sys).
Usage: RTRE_SYS=/path/to/driver.sys python -c "from rtre import *; print(show(0x140001000))"
: functions from .pdata, rip-relative xrefs, call graph."""
import bisect, pickle, struct, sys, os
import pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
from capstone.x86 import X86_OP_MEM, X86_OP_IMM, X86_REG_RIP

SYS = os.environ['RTRE_SYS']   # path to the Windows driver (.sys) to analyse
CACHE = os.environ.get('RTRE_CACHE', os.path.join(os.path.expanduser('~/.cache'), os.path.basename(SYS) + '.rtre.pkl'))

pe = pefile.PE(SYS, fast_load=False)
BASE = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image()

def va2off(va):
    return pe.get_offset_from_rva(va - BASE)
def off2va(off):
    return BASE + pe.get_rva_from_offset(off)
def rd(va, n):
    r = va - BASE
    return img[r:r + n]
def u32(va): return struct.unpack('<I', rd(va, 4))[0]
def u16(va): return struct.unpack('<H', rd(va, 2))[0]
def u64(va): return struct.unpack('<Q', rd(va, 8))[0]
def wstr(va, maxn=200):
    b = rd(va, maxn * 2)
    s = b.decode('utf-16le', 'replace')
    return s.split('\0')[0]
def astr(va, maxn=200):
    return rd(va, maxn).split(b'\0')[0].decode('latin1')

def functions():
    funcs = []
    for e in pe.DIRECTORY_ENTRY_EXCEPTION:
        funcs.append((BASE + e.struct.BeginAddress, BASE + e.struct.EndAddress))
    funcs = sorted(set(funcs))
    return funcs

md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = True

def disasm(start, end):
    return list(md.disasm(rd(start, end - start), start))

def build():
    funcs = functions()
    xrefs = {}   # target va -> [(insn va, func start, mnemonic)]
    calls = {}   # func start -> set(call targets)
    for fs, fe in funcs:
        cs = set()
        for i in md.disasm(rd(fs, fe - fs), fs):
            for op in i.operands:
                if op.type == X86_OP_MEM and op.mem.base == X86_REG_RIP:
                    t = i.address + i.size + op.mem.disp
                    xrefs.setdefault(t, []).append((i.address, fs, i.mnemonic))
                elif op.type == X86_OP_IMM and i.mnemonic in ('call', 'jmp') and i.group(1) or (op.type == X86_OP_IMM and i.mnemonic == 'call'):
                    cs.add(op.imm)
                    xrefs.setdefault(op.imm, []).append((i.address, fs, i.mnemonic))
        calls[fs] = cs
    pickle.dump((funcs, xrefs, calls), open(CACHE, 'wb'))
    return funcs, xrefs, calls

def load():
    if os.path.exists(CACHE):
        return pickle.load(open(CACHE, 'rb'))
    return build()

FUNCS, XREFS, CALLS = load()
_FS = [f[0] for f in FUNCS]

def func_of(va):
    i = bisect.bisect_right(_FS, va) - 1
    if i >= 0 and FUNCS[i][0] <= va < FUNCS[i][1]:
        return FUNCS[i]
    return None

def show(fs, limit=400, annotate=True):
    f = func_of(fs)
    out = []
    for i in disasm(f[0], f[1])[:limit]:
        note = ''
        if annotate:
            for op in i.operands:
                if op.type == X86_OP_MEM and op.mem.base == X86_REG_RIP:
                    t = i.address + i.size + op.mem.disp
                    try:
                        w = wstr(t, 60); a = astr(t, 60)
                        if len(w) >= 3 and w.isprintable(): note = f'  ; L"{w}"'
                        elif len(a) >= 4 and a.isprintable(): note = f'  ; "{a}"'
                        else: note = f'  ; [{t:#x}]'
                    except Exception: pass
        out.append(f'{i.address:#x}: {i.mnemonic} {i.op_str}{note}')
    return '\n'.join(out)

if __name__ == '__main__':
    print(len(FUNCS), 'functions;', len(XREFS), 'xref targets')
