import os
import array, math, subprocess, time, wave, sys
exec(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "micsweep.py")).read().split("tone(\"/tmp/t1k.wav\")")[0])
SRC = "alsa_input.pci-0000_00_1f.3.analog-stereo"
tone("/tmp/t1k.wav", amp=12000)
subprocess.run(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "0.5"])
def hw():
    o = subprocess.run(["sudo", "amixer", "-c0", "cget", "name=Internal Mic Boost Volume"], capture_output=True, text=True).stdout
    b = o.strip().splitlines()[-1].split("=")[1]
    o = subprocess.run(["sudo", "amixer", "-c0", "cget", "name=Capture Volume"], capture_output=True, text=True).stdout
    return b, o.strip().splitlines()[-1].split("=")[1]
print(f"{'src%':>5} {'boost':>6} {'cap':>8} | {'tone1k':>7} {'peak':>6} {'clip%':>6} {'LF<40Hz':>8}")
for pct in (30, 40, 50, 60, 70, 80, 100):
    subprocess.run(["pactl", "set-source-volume", SRC, f"{pct}%"]); time.sleep(0.3)
    p = rec("/tmp/s.wav", 2.5); time.sleep(0.3); subprocess.run(["pw-play", "/tmp/t1k.wav"]); p.wait()
    l, _ = load("/tmp/s.wav"); seg = l[R//2:R*2]
    lf = max(db(goertzel(seg[:24000], f)) for f in (10, 20, 30))
    pk = max(abs(v) for v in seg); clip = sum(1 for v in seg if abs(v) >= 32000)/len(seg)*100
    b, c = hw()
    print(f"{pct:>5} {b:>6} {c:>8} | {db(goertzel(seg,1000)):>7.1f} {db(pk):>6.1f} {clip:>6.2f} {lf:>8.1f}", flush=True)
