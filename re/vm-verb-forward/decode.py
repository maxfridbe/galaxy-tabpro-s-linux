import sys
"""Decode forwarded HDA verbs into coefficient and bridge transactions."""
L=[l.split() for l in open(sys.argv[1]) if l[:1].isdigit()]
idx=None; reg={}; out=[]
for t,nid,v,p,_,r in L:
    v=int(v,16); p=int(p,16); r=int(r,16); full=(v<<8)|p
    if nid!="20":
        continue
    if (full>>16)==0x5: idx=full&0xffff; continue
    if (full>>16)==0x4:
        val=full&0xffff; reg[idx]=val
        if idx==0x26:
            cmd=val; dev=reg.get(0x22); rr=reg.get(0x23); d=reg.get(0x25); hi=reg.get(0x89,0)
            kind={0xb010:"W8",0xb018:"R8",0xb013:"W16",0xb01b:"R16",0xb033:"W32r",0xb03b:"R32r",0x8010:"W8",0x8013:"W16"}.get(cmd & 0xb03f if cmd&0x8000 else cmd, f"cmd{cmd:#06x}")
            if cmd & 0x8000 and not (cmd & 0x4000) or cmd in (0xb010,0xb013,0xb033,0xb018,0xb01b,0xb03b):
                out.append(f"{t} BRIDGE {kind} dev={(dev or 0):#04x} reg={(hi<<16|(rr or 0)):#06x}" + (f" val={(d or 0):#06x}" if 'W' in kind else ""))
        elif idx not in (0x22,0x23,0x25,0x89,0x24):
            out.append(f"{t} COEF_W {idx:#04x}={val:#06x}")
    elif (full>>16)==0xc:
        if idx==0x28: out.append(f"{t}   READ -> {r:#06x}")
        elif idx not in (0x26,): out.append(f"{t} COEF_R {idx:#04x}->{r:#06x}")
print("\n".join(out))
