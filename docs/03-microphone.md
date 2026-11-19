# 03 – Internal microphone

The internal microphone works out of the box (digital mic on the ALC298, pin `0x12`,
"Internal Mic"). The firmware's NHLT table is empty, so the DSP (SOF/SST) is not involved and the
plain `snd-hda-intel` driver is correct.

## The one problem: default level = clipping
PipeWire/ALSA maps the source volume slider over **both** hardware gain stages
(*Internal Mic Boost* 0–3 × 10 dB and *Capture* −33.5 … +30 dB). At the default **100 %** both are
at maximum and very-low-frequency rumble alone drives the ADC into clipping — recordings sound like
loud static.

**Set the input level to about 50 %** (GNOME Settings → Sound → Input, or
`pactl set-source-volume alsa_input.pci-0000_00_1f.3.analog-stereo 50%`). WirePlumber remembers it.
Do not exceed ~60 %.

## Measurements (speaker → mic loopback, 1 kHz tone)
Tone level tracked the speaker volume over ~70 dB (−100 dBFS muted → −28 dBFS at 90 %), a control
frequency (1.3 kHz) stayed at the noise floor, left/right agree (correlation 0.93):

| source volume | peak | clipped samples |
|---|---|---|
| 30 % | −17 dBFS | 0 % |
| 50 % | −4 dBFS | 0 % |
| 60 % | 0 dBFS | 6 % |
| 100 % (default) | 0 dBFS | 81 % |

(speaker at 50 %, tablet's own speaker right next to the mic — far louder than a voice.)

A little rumble below 40 Hz remains about 30 dB under the signal at 50 %. A PipeWire high-pass
filter would remove it if it bothers you.

Scripts used: `re/micsweep.py`, `re/miccheck.py`, `re/micvol.py`.
