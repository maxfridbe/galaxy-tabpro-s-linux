import sys; import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from hdav import *
dev = int(sys.argv[1], 0)
vals = [br(dev, 1, r) for r in range(256)]
hi = [v >> 8 for v in vals]
print("t1 raw (16-bit):"); 
for row in range(0, 256, 16): print(f"{row:02x}: " + " ".join(f"{v:04x}" for v in vals[row:row+16]))
print("t2 regs 0..0x1f:", " ".join(f"{br(dev,2,r):04x}" for r in range(0x20)))
