import array, math, subprocess, time, wave, sys
import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); exec(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "micsweep.py")).read().split("tone(\"/tmp/t1k.wav\")")[0])
amix("Internal Mic Boost Volume", 0); amix("Capture Volume", 80)
p = rec("/tmp/n.wav", 2); p.wait(); l, r = load("/tmp/n.wav"); l = l[4800:]; r = r[4800:]
print("noise spectrum L (dBFS at f):", " ".join(f"{f}:{db(goertzel(l[:24000], f)):.0f}" for f in (50, 100, 300, 1000, 3000, 6000, 10000, 16000)))
print("L/R correlation:", round(sum(a*b for a, b in zip(l, r))/math.sqrt(sum(a*a for a in l)*sum(b*b for b in r)), 3))
print("noise rms L/R:", round(db(math.sqrt(sum(v*v for v in l)/len(l))), 1), round(db(math.sqrt(sum(v*v for v in r)/len(r))), 1))
tone("/tmp/t1k.wav", amp=12000)
for vol in ("0.0", "0.3", "0.6", "0.9"):
    subprocess.run(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", vol])
    p = rec("/tmp/s.wav", 2.5); time.sleep(0.3); subprocess.run(["pw-play", "/tmp/t1k.wav"]); p.wait()
    l, r = load("/tmp/s.wav"); seg = l[R//2:R*2]; segr = r[R//2:R*2]
    print(f"speaker vol {vol}: 1k tone L {db(goertzel(seg,1000)):.1f} R {db(goertzel(segr,1000)):.1f}  | 1.3k (control) {db(goertzel(seg,1300)):.1f}")
subprocess.run(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "0.4"])
