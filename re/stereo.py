import sys; import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from hdav import *
# MAX98925 reg 0x2D: [6:5] DAC_IN_SEL (0=left DAI,1=right DAI,2=sum,3=sum/2), [4:0] SPK_GAIN
mode = sys.argv[1]
for dev, ch in ((0x31, 0), (0x34, 1)):
    old = r8(dev, 0x2d)
    new = (old & 0x1f) | ((3 if mode == 'mono' else ch) << 5)
    w8(dev, 0x2d, new)
    print(f"{dev:#04x}: 0x2d {old:#04x} -> {r8(dev,0x2d):#04x}")
