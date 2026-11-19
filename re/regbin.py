import sys,re
t=open(sys.argv[1],'rb').read().decode('utf-16')
key=None; out={}
lines=t.replace('\\\r\n','').replace('\\\n','').splitlines()
for l in lines:
    l=l.strip()
    if l.startswith('['): key=l[1:-1]; continue
    m=re.match(r'"([^"]*)"=hex(?:\(\w+\))?:(.*)',l)
    if m and key and '\\ACPI\\' in key:
        b=bytes(int(x,16) for x in m.group(2).replace(' ','').split(',') if x)
        if len(b)>36 and b[:4].isalpha():
            out.setdefault(b[:4].decode(),b)
for k,v in out.items():
    open(f'{sys.argv[2]}/{k}-reg.bin','wb').write(v); print(k,len(v))
