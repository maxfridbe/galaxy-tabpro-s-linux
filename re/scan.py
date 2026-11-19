import sys; import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from hdav import *
found = []
for dev in range(0x01, 0x100):
    r = {t: br(dev, t, 0x00) for t in (1, 2)}
    r3 = br(dev, 3, 0x0000)
    if any(v not in (0xffff, 0xff) for v in list(r.values()) + [r3]):
        found.append(dev); print(f"dev {dev:#04x}: t1 reg0={r[1]:#06x} t2 reg0={r[2]:#06x} t3 reg0={r3:#06x}")
print("responding:", [hex(d) for d in found])
