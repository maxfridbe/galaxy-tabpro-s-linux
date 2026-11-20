#!/usr/bin/python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Execute HD Audio verbs forwarded by QEMU's hda-forward codec on the real codec and log them.

Runs as root on the tablet, listens on 127.0.0.1:7171 (reach it through `ssh -L`).
Request {u32 nid, u32 data20}, reply {u32 response}. Every verb is appended to the log as
  <seconds> <nid> <verb12> <param8> -> <response>
"""
import fcntl, os, socket, struct, sys, time

DEV = '/dev/snd/hwC0D0'
IOC = 0xC0084811            # HDA_IOCTL_VERB_WRITE
LOG = sys.argv[1] if len(sys.argv) > 1 else '/var/tmp/hda-verbs.log'

fd = os.open(DEV, os.O_RDWR)
def verb(nid, data):
    cmd = (nid << 24) | (data & 0xfffff)           # HDA_VERB(nid, verb, param) layout
    return struct.unpack('II', fcntl.ioctl(fd, IOC, struct.pack('II', cmd, 0)))[1]

def recvall(c, n):
    b = b''
    while len(b) < n:
        x = c.recv(n - len(b))
        if not x:
            return None
        b += x
    return b

srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(('127.0.0.1', 7171)); srv.listen(1)
print(f'hda-verb-server: {DEV}, logging to {LOG}', flush=True)
log = open(LOG, 'a', buffering=1)
while True:
    c, peer = srv.accept()
    c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    t0 = time.time(); n = 0
    log.write(f'# connect {time.ctime()}\n')
    print('client connected', flush=True)
    while True:
        b = recvall(c, 8)
        if b is None:
            break
        nid, data = struct.unpack('<II', b)
        try:
            r = verb(nid & 0x7f, data)
        except OSError as e:
            r = 0
            log.write(f'# ioctl error {e}\n')
        c.sendall(struct.pack('<I', r))
        n += 1
        log.write(f'{time.time() - t0:.4f} {nid:02x} {(data >> 8) & 0xfff:03x} {data & 0xff:02x} -> {r:08x}\n')
    log.write(f'# disconnect after {n} verbs\n')
    print(f'client gone after {n} verbs', flush=True)
    c.close()
