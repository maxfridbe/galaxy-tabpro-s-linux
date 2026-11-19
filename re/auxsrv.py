"""Line protocol AUX server: W <addr> <hex> / R <addr> <len> on /dev/drm_dp_aux0 (root). Used by samo-profile.py."""
import os, sys
fd = os.open("/dev/drm_dp_aux0", os.O_RDWR)
for line in sys.stdin:
    p = line.split()
    try:
        if p[0] == 'W': os.pwrite(fd, bytes.fromhex(p[2]), int(p[1])); print('ok', flush=True)
        else: print(os.pread(fd, int(p[2]), int(p[1])).hex() or '-', flush=True)
    except Exception as e: print('ERR', e, flush=True)
