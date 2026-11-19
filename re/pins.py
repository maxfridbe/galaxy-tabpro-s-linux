import sys; import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from hdav import *
nid = int(sys.argv[1], 0); pinctl = int(sys.argv[2], 0); conn = int(sys.argv[3], 0)
verb(nid, 0x701, conn)        # connection select
verb(nid, 0x707, pinctl)      # pin widget control (0x40 OUT, 0xc0 OUT+HP)
verb(nid, 0x300, 0xb000)      # amp out: both ch, unmute, gain 0
verb(nid, 0x70c, 0x02)        # EAPD on
print(hex(nid), "pinctl", hex(verb(nid, 0xf07, 0)), "conn", hex(verb(nid, 0xf01, 0)), "amp", hex(verb(nid, 0xb00, 0xa000)), hex(verb(nid, 0xb00, 0x8000)))
