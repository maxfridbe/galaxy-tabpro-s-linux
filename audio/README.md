# audio/ – internal speaker fix

| file | |
|---|---|
| `w700-speakers` | Python 3 script (no dependencies). Powers up the ALC298's internal I2S block (bridge device 0x1b) and enables the two MAX98925 amps (0x31 left, 0x34 right). Refuses to run on other hardware. |
| `w700-speakers.service` | runs it at boot |
| `w700-speakers-resume.service` | runs it after suspend/hibernate |
| `w700-speakers.conf` | → `/etc/w700-speakers.conf`, `GAIN=0..31` (dB = GAIN−6) |
| `install.sh` | installs all of the above + disables HDA power saving (modprobe + tuned) |

```
sudo ./audio/install.sh              # install / reinstall
sudo ./audio/install.sh --uninstall
journalctl -u w700-speakers          # "w700-speakers: enabled 0x31:en=0x80,gain=0x1c 0x34:..."
```
Background: [docs/02-audio-speakers.md](../docs/02-audio-speakers.md).
