import sys, zlib, struct, numpy as np
path, W, H, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
d = np.fromfile(path, dtype=np.uint8)
stride = int(sys.argv[5]) if len(sys.argv) > 5 else W
fs = stride * H * 3 // 2
d = d[-fs:] if len(d) >= fs else d
y = d[:stride * H].reshape(H, stride)[:, :W].astype(np.float32)
uv = d[stride * H:stride * H + stride * H // 2].reshape(H // 2, stride)[:, :W].astype(np.float32)
u = np.repeat(np.repeat(uv[:, 0::2], 2, 0), 2, 1)[:H, :W] - 128
v = np.repeat(np.repeat(uv[:, 1::2], 2, 0), 2, 1)[:H, :W] - 128
rgb = np.stack([y + 1.402 * v, y - 0.344 * u - 0.714 * v, y + 1.772 * u], 2)
img = np.clip(rgb, 0, 255).astype(np.uint8)[::2, ::2]
h, w, _ = img.shape
print(f"Y mean {y.mean():.1f} min {y.min():.0f} max {y.max():.0f}; U {u.mean():+.1f} V {v.mean():+.1f}")
raw = b''.join(b'\x00' + img[r].tobytes() for r in range(h))
c = lambda t, x: struct.pack('>I', len(x)) + t + x + struct.pack('>I', zlib.crc32(t + x) & 0xffffffff)
open(out, 'wb').write(b'\x89PNG\r\n\x1a\n' + c(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + c(b'IDAT', zlib.compress(raw, 6)) + c(b'IEND', b''))
