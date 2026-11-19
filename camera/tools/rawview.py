"""Unpack IPU3 packed RAW10 frames, report stats, write a half-res debayered preview PNG."""
import sys, zlib, struct
import numpy as np
path, W, H = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
order = sys.argv[4] if len(sys.argv) > 4 else 'RGGB'
out = sys.argv[5] if len(sys.argv) > 5 else '/tmp/preview.png'
bpl = ((W + 24) // 25) * 32
frame_bytes = bpl * H
data = np.fromfile(path, dtype=np.uint8)
n = len(data) // frame_bytes
f = data[(n - 1) * frame_bytes:n * frame_bytes].reshape(H, bpl // 32, 32)   # last frame
bits = np.unpackbits(f, axis=2, bitorder='little')[:, :, :250].reshape(H, -1, 25, 10)
px = (bits * (1 << np.arange(10))).sum(axis=3).reshape(H, -1)[:, :W].astype(np.float32)
print(f"frames={n} size={W}x{H} min={px.min():.0f} max={px.max():.0f} mean={px.mean():.1f}")
ch = {'00': px[0::2, 0::2], '01': px[0::2, 1::2], '10': px[1::2, 0::2], '11': px[1::2, 1::2]}
for k, v in ch.items(): print(f"  bayer pos {k}: mean {v.mean():7.1f}  std {v.std():6.1f}")
pos = {'RGGB': ('00', '01', '10', '11'), 'BGGR': ('11', '01', '10', '00'), 'GRBG': ('01', '00', '11', '10'), 'GBRG': ('10', '00', '11', '01')}[order]
r, g1, g2, b = (ch[p] for p in pos)
g = (g1 + g2) / 2
blk = 64.0
rgb = np.stack([r, g, b], axis=2) - blk
rgb = np.clip(rgb, 0, None)
avg = rgb.reshape(-1, 3).mean(axis=0) + 1e-6
rgb *= avg[1] / avg                      # grey-world white balance
rgb /= max(np.percentile(rgb, 99.5), 1)  # auto exposure for preview
rgb = np.clip(rgb, 0, 1) ** (1 / 2.2)
img = (rgb * 255).astype(np.uint8)
h, w, _ = img.shape
raw = b''.join(b'\x00' + img[y].tobytes() for y in range(h))
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b'')
open(out, 'wb').write(png)
print("preview", out, f"{w}x{h}", "wb gains", np.round(avg[1] / avg, 2))
