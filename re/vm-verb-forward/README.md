# Recording what a Windows codec driver does to the real hardware

`hda-forward.c` is a QEMU HD Audio codec device that forwards every verb the guest sends to a
helper on the real machine (`hda-verb-server.py`, which executes it through
`/dev/snd/hwC0D0` / `HDA_IOCTL_VERB_WRITE` and logs it) and returns the real codec's answer. It also
consumes the guest's audio stream at the real rate (data is discarded) so playback runs.

`qemu-intel-hda.patch` adds it to QEMU 10.0's build and lets the emulated ICH9 controller present
itself as Sunrise Point-LP HD Audio (8086:9D70, subsystem 144D:C135) when `HDA_SPT=1` is set — the
TabPro S Realtek driver only creates its Samsung amp object on that controller.

```
# tablet (root): python3 hda-verb-server.py /var/tmp/hda-verbs.log      (listens on 127.0.0.1:7171)
# host:          ssh -N -L 7171:127.0.0.1:7171 tablet
#                HDA_SPT=1 qemu-system-x86_64 ... -device ich9-intel-hda,addr=0x1b \
#                    -device hda-forward,host=127.0.0.1,port=7171
# guest:         boot with driver signature enforcement disabled, install the Realtek INF with its
#                hardware IDs changed from INTELAUDIO\ to HDAUDIO\ (Device Manager, pick from list)
python3 decode.py hda-verbs.log      # coef + bridge transactions
```
Use a throwaway disk overlay for the VM. The helper holds the codec's hwdep device exclusively;
stop it before using other tools on the tablet, and re-bind `snd_hda_intel` afterwards (the guest
reprograms converter stream assignments).
