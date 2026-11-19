#!/usr/bin/python3
# tiny hda-verb: talk to the ALC298 through /dev/snd/hwC0D0 (needs seat ACL, no root)
import fcntl, os, struct, sys, time
FD = os.open('/dev/snd/hwC0D0', os.O_RDWR)
IOC = 0xC0084811  # HDA_IOCTL_VERB_WRITE
def verb(nid, v, p=0):
    cmd = (nid << 24) | (v << 8) | p  # HDA_VERB() layout from sound/hda_hwdep.h
    return struct.unpack('II', fcntl.ioctl(FD, IOC, struct.pack('II', cmd, 0)))[1]
def coef_w(idx, val, nid=0x20): verb(nid, 0x500, idx); verb(nid, 0x400, val)
def coef_r(idx, nid=0x20): verb(nid, 0x500, idx); return verb(nid, 0xc00, 0)
def pack(reg, val): coef_w(0x23, reg); coef_w(0x25, val); coef_w(0x26, 0xb011)
def gpio(mask, data):  # AFG node 0x01
    verb(0x01, 0x716, mask); verb(0x01, 0x717, mask); verb(0x01, 0x715, data)
V1_INIT = [(0x19,0x00),(0x20,0xc0),(0x22,0x44),(0x23,0x08),(0x24,0x85),(0x25,0x41),(0x35,0x40),(0x36,0x01),(0x38,0x81),(0x3a,0x03),(0x3b,0x81),(0x40,0x3e),(0x41,0x07),(0x400,0x1)]
V1_AMPS = {0x3a: [(0x18,0x1),(0x26,0x0)], 0x39: [(0x18,0x2),(0x26,0x1)]}
V2_BASE = [(0x23e1,0),(0x2012,0x6f),(0x2014,0),(0x201b,1),(0x201d,1),(0x201f,0xfe),(0x2021,0),(0x2022,0x10),(0x203d,5),(0x203f,3),(0x2050,0x2c),(0x2076,0xe),(0x207c,0x4a),(0x2081,3)]
def v1(addrs):
    for a in addrs:
        coef_w(0x22, a)
        for r, v in V1_AMPS.get(a, [(0x18,0x1),(0x26,0x0)]): pack(r, v)
        for r, v in V1_INIT: pack(r, v)
def v2(addrs, ch=None):
    for i, a in enumerate(addrs):
        coef_w(0x22, a)
        seq = list(V2_BASE)
        if i % 2:  # right-channel variant from the kernel table
            seq = [(r, {0x201b:2,0x201d:2,0x201f:0xfd,0x2021:1}.get(r, v)) for r, v in seq]
        for r, v in seq + [(0x23ba,0x94)]: pack(r, v)
        coef_w(0x89, 0)
        pack(0x203a, 0x81); pack(0x23ff, 1)
if __name__ == '__main__':
    cmd = sys.argv[1]
    a = [int(x, 0) for x in sys.argv[2:]]
    if cmd == 'gpio': gpio(a[0], a[1])
    elif cmd == 'v1': v1(a)
    elif cmd == 'v2': v2(a)
    elif cmd == 'coefr': print(hex(coef_r(a[0])))
    elif cmd == 'verb': print(hex(verb(*a)))

# ---- Windows RTKVHD64 smart-amp bridge protocol (ALC298), reverse-engineered ----
B_ADDR, B_REG, B_DATA, B_CMD, B_RDATA, B_REGHI = 0x22, 0x23, 0x25, 0x26, 0x28, 0x89
WCMD = {1: 0xb012, 2: 0xb013, 3: 0xb033}
RCMD = {1: 0xb01a, 2: 0xb01b, 3: 0xb03b}
def _wait():
    for _ in range(100):
        if not coef_r(B_CMD) & 0x4000: return True
        time.sleep(0.001)
    return False
def _start(): coef_w(B_CMD, coef_r(B_CMD) | 0x4000)
def bw(dev, typ, reg, val):
    _wait(); _start()
    coef_w(B_ADDR, dev); coef_w(B_DATA, val)
    if typ == 3: coef_w(B_REGHI, reg >> 16)
    coef_w(B_REG, reg & 0xffff); coef_w(B_CMD, WCMD[typ])
def br(dev, typ, reg):
    _wait(); _start()
    coef_w(B_ADDR, dev)
    if typ == 3: coef_w(B_REGHI, reg >> 16)
    coef_w(B_REG, reg & 0xffff); coef_w(B_CMD, RCMD[typ])
    _wait(); time.sleep(0.002)
    return coef_r(B_RDATA)

def w8(dev, reg, val):  # 8-bit reg / 8-bit data (cmd 0xb010), as RTKVHD64 0x464cc
    _wait(); _start(); coef_w(B_ADDR, dev); coef_w(B_DATA, val); coef_w(B_REG, reg); coef_w(B_CMD, 0xb010); _wait()
def r8(dev, reg):       # cmd 0xb018, as RTKVHD64 0x4664c
    _wait(); _start(); coef_w(B_ADDR, dev); coef_w(B_REG, reg); coef_w(B_CMD, 0xb018); _wait(); time.sleep(0.002)
    return coef_r(B_RDATA) & 0xff
