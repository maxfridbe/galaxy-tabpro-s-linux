# 08 – Power, suspend and misc

## Suspend
`/sys/power/mem_sleep` offers `[s2idle] deep`; s2idle works, the power button wakes the tablet and
the speakers come back (`w700-speakers-resume.service`). Deep (S3) is untested.

## Disable automatic suspend but keep manual suspend
```
gsettings set org.gnome.settings-daemon.plugins.power sleep-inactive-ac-type nothing
gsettings set org.gnome.settings-daemon.plugins.power sleep-inactive-battery-type nothing
gsettings set org.gnome.settings-daemon.plugins.power idle-dim true          # screen may still dim/blank
gsettings set org.gnome.desktop.screensaver lock-enabled false               # no auto-lock on idle
```
(Do **not** mask `suspend.target` if you still want the menu's Suspend.)

## Power button → lock + screen off instead of suspend
GNOME has no "lock" option for the power button, so set it to *nothing* and add a custom shortcut on
`XF86PowerOff`:
```
gsettings set org.gnome.settings-daemon.plugins.power power-button-action nothing
P=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/powerlock/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$P']"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$P name 'Power button: lock'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$P command \
  'dbus-send --session --type=method_call --dest=org.gnome.ScreenSaver /org/gnome/ScreenSaver org.gnome.ScreenSaver.Lock'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$P binding 'XF86PowerOff'
```
Back to suspend: `gsettings set org.gnome.settings-daemon.plugins.power power-button-action suspend`
and `gsettings reset org.gnome.settings-daemon.plugins.media-keys custom-keybindings`.

## tuned and audio power saving
Fedora's `tuned` `balanced`/`powersave` profiles set `[audio] timeout=10`, i.e. the HDA codec
suspends after 10 s idle — which drops the speaker configuration. `audio/install.sh` installs
overrides in `/etc/tuned/profiles/` with `timeout=0`. If you switch power profiles and the speakers
go silent, run `sudo /usr/local/sbin/w700-speakers`.

## Clock
The RTC on our unit was two years behind; set the time in the firmware or Windows before installing
(HTTPS fails otherwise). Linux keeps it correct afterwards via NTP.

## Firmware hot keys
F2/F10/Del at power-on were unreliable. Use Windows *Advanced startup* (Shift+Restart), or
`systemctl reboot --firmware-setup` from Linux.
