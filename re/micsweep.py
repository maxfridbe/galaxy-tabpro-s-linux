"""Speaker->mic loopback sweep: for each (boost, capture) measure noise floor and 1 kHz tone level."""
import array, math, os, subprocess, sys, time, wave
R = 48000
def tone(path, f=1000, secs=2.5, amp=6000):
    w = wave.open(path, "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(R)
    w.writeframes(b"".join(int(amp*math.sin(2*math.pi*f*i/R)).to_bytes(2,"little",signed=True)*2 for i in range(int(R*secs)))); w.close()
def rec(path, secs):
    return subprocess.Popen(["timeout", str(secs), "pw-record", "--rate", str(R), "--channels", "2", "--format", "s16", path])
def load(path):
    w = wave.open(path); a = array.array("h", w.readframes(w.getnframes())); return a[0::2], a[1::2]
def goertzel(x, f):
    k = 2*math.cos(2*math.pi*f/R); s1 = s2 = 0.0
    for v in x: s1, s2 = v + k*s1 - s2, s1
    p = s1*s1 + s2*s2 - k*s1*s2
    return math.sqrt(max(p, 0))*2/len(x)
def db(v): return 20*math.log10(max(v, 1e-9)/32768)
def amix(name, v): subprocess.run(["sudo", "amixer", "-q", "-c0", "cset", f"name={name}", f"{v},{v}"], check=True)
tone("/tmp/t1k.wav")
print(f"{'boost':>5} {'cap':>4} | {'noise rms':>9} {'tone 1k':>8} {'SNR':>6} {'peak':>6} {'clip%':>6}")
for boost in (0, 1, 2, 3):
    for cap in (63, 80, 95, 110, 127):
        amix("Internal Mic Boost Volume", boost); amix("Capture Volume", cap); time.sleep(0.2)
        p = rec("/tmp/n.wav", 1.5); p.wait()
        nl, _ = load("/tmp/n.wav"); nl = nl[4800:]
        noise = math.sqrt(sum(v*v for v in nl)/len(nl))
        p = rec("/tmp/s.wav", 2.5); time.sleep(0.3); subprocess.run(["pw-play", "/tmp/t1k.wav"]); p.wait()
        sl, _ = load("/tmp/s.wav"); seg = sl[R//2:R*2]
        t = goertzel(seg, 1000); pk = max(abs(v) for v in seg); clip = sum(1 for v in seg if abs(v) >= 32000)/len(seg)*100
        print(f"{boost:>5} {cap:>4} | {db(noise):>8.1f} {db(t):>8.1f} {db(t)-db(noise):>6.1f} {db(pk):>6.1f} {clip:>6.2f}", flush=True)
