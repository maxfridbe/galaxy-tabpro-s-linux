#!/usr/bin/env python3
"""Generate the per-panel AMOLED brightness profile used by brightness/tabpro-s-brightness.

Runs Samsung's SAMOPanel.sys init (emulated with Unicorn) against the real panel once, so the TCON
returns this unit's calibration data, then computes all 101 brightness levels offline.

  RTRE_SYS=/path/to/SAMOPanel.sys python3 samo-profile.py [--ssh user@tablet] -o amoled-profile.json

Without --ssh it must run on the tablet itself (uses sudo for /dev/drm_dp_aux0). SAMOPanel.sys comes
from your own Windows install (C:\\Windows\\System32\\drivers) or Samsung's driver package; it is not
redistributed here. The init writes go to the live panel (the same writes Windows does at boot).
"""
import argparse, json, os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emusamo
from unicorn.x86_const import UC_X86_REG_R8

INIT = 0x140006110          # panel-interface event handler; event 7 / arg 1 = full init + level 100
OUT_BUF = 0x2000_0000 + 0x80000

ap = argparse.ArgumentParser()
ap.add_argument('--ssh', help='run the AUX accesses on this host over ssh')
ap.add_argument('-o', '--output', default='amoled-profile.json')
a = ap.parse_args()

srv = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'auxsrv.py')
if a.ssh:
    subprocess.run(['scp', '-q', srv, f'{a.ssh}:/tmp/auxsrv.py'], check=True)
    cmd = ['ssh', a.ssh, 'sudo python3 /tmp/auxsrv.py']
else:
    cmd = ['sudo', sys.executable, srv]
emusamo.LIVE = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

# 1. live init: unlock ("PARAAUX-REG"), read calibration, program the panel
live = emusamo.run(7, ctx_fields={0xbfa: 1, 0x2ad: 0}, entry=INIT, regs={UC_X86_REG_R8: 1})
emusamo.LIVE.stdin.close(); emusamo.LIVE.wait(); emusamo.LIVE = None
if any(e[0] in ('EXC', 'ERR') for e in live):
    raise SystemExit(f'init failed: {[e for e in live if e[0] in ("EXC", "ERR")][:3]}')
reads = [e[3] for e in live if e[0] == 'R']
if not any(int(r, 16) for r in reads):
    raise SystemExit('panel returned only zeros: TCON did not unlock (wrong device?)')

# 2. offline: same init with the recorded reads, then every level
log = emusamo.run(7, ctx_fields={0xbfa: 1, 0x2ad: 0}, entry=INIT, regs={UC_X86_REG_R8: 1}, reads=list(reads),
                  then=[(emusamo.SET_BRIGHTNESS, lv, OUT_BUF) for lv in range(101)])
segs = [[]]
for e in log:
    if e[0] == 'CALL': segs.append([])
    elif e[0] in ('W', 'R', 'SLEEP_US'): segs[-1].append(e)
    elif e[0] == 'EXC': raise SystemExit(f'emulation exception {e}')
enc = lambda s: [['W', e[2], e[3]] if e[0] == 'W' else ['R', e[2], len(e[3]) // 2] if e[0] == 'R' else ['S', e[1]]
                 for e in s]
prof = {'init': enc(segs[0]), 'levels': [enc(s) for s in segs[1:]]}
assert len(prof['levels']) == 101
json.dump(prof, open(a.output, 'w'))
print(f'wrote {a.output}: init {len(prof["init"])} ops, 101 levels; install to /etc/tabpro-s/amoled-profile.json')
