import sys; import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from hdav import *
for dev in (0x31, 0x34):
    print(f"== dev {dev:#04x}")
    for reg in (0x21ff, 0x24ff, 0x2000, 0x2001, 0x2002, 0x203a, 0x203d, 0x2040, 0x20ff, 0x23ff, 0x2012, 0x2014, 0x201b, 0x201d, 0x201f, 0x2021, 0x2022):
        print(f"  t3 reg {reg:#06x} = {br(dev,3,reg):#06x}")
    print("  t1 regs 0x00-0x0f:", " ".join(f"{br(dev,1,r):02x}" for r in range(16)))
