#!/usr/bin/env bash
# Builds a ready-to-flash Raspberry Pi OS image with plv preinstalled.
#
#   sudo ./build_image.sh <raspios-lite-armhf.img[.xz]> [plv-binary] [output.img]
#
# Base: 32-bit Raspberry Pi OS Lite (Bookworm or later), as downloaded; it is
# never modified. Defaults: binary target/armv7-unknown-linux-gnueabihf/release/plv,
# output plv-<date>.img. Environment:
#   SSH_KEY           public key line for user $PI_USER (Imager can't customise a
#                     custom image, so this is how you get in), e.g.
#                     SSH_KEY="$(cat ~/.ssh/id_ed25519.pub)"
#   PI_USER           default plv;  PI_HOSTNAME  default pianoledvisualizer
#   WIFI_COUNTRY      default GB; Wi-Fi stays blocked until a country is set
#   WIFI_SSID, WIFI_PASSWORD  home network (optional: else join the hotspot
#                     and pick it in the app)
#   HOTSPOT_PASSWORD  default random;  SONGS_DIR  songs to preload
#   STATUS_LOG=1      write network and service status to plv-status.txt on
#                     the boot partition every 2 min (troubleshooting)
#
# Linux, as root. On Windows/macOS run it in Docker:
#   docker run --rm --privileged -v "$PWD:/src" -w /src plv-armv7 ./build_image.sh <base>
#
# Flash the result with Raspberry Pi Imager (Use custom). No chroot or ARM
# emulation: everything is copied in as files, first-boot setup via cloud-init.
set -euo pipefail

BASE=${1:?usage: $0 <raspios-lite-armhf.img[.xz]> [plv-binary] [output.img]}
BIN=${2:-target/armv7-unknown-linux-gnueabihf/release/plv}
OUT=${3:-plv-$(date +%Y%m%d).img}
HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
HOTSPOT=PianoLED-Setup
PI_USER=${PI_USER:-plv}
PI_HOSTNAME=${PI_HOSTNAME:-pianoledvisualizer}
WIFI_COUNTRY=${WIFI_COUNTRY:-GB}
WIFI_PASSWORD=${WIFI_PASSWORD:-}

die() { echo "error: $*" >&2; exit 1; }
# Copies a text file with LF line endings: a Windows checkout may have CRLF,
# which breaks systemd units and scripts on the Pi.
put_text() { tr -d '\r' <"$2" >"$3"; chmod "$1" "$3"; }
step() { echo "== $*"; }

for tool in sfdisk mount umount install; do
  command -v "$tool" >/dev/null || die "missing tool: $tool"
done
[ "$(id -u)" -eq 0 ] || die "run as root (it loop-mounts the image)"
[ -f "$BASE" ] || die "no base image at '$BASE'"
[ -f "$BIN" ] || die "no plv binary at '$BIN' (see README, Build)"
[ -f "$HERE/systemd/plv.service" ] || die "run from a checkout of the repo"
[[ $WIFI_COUNTRY =~ ^[A-Z]{2}$ ]] || die "WIFI_COUNTRY must be a two-letter code like NL"
[ -z "${WIFI_SSID:-}" ] || [ ${#WIFI_PASSWORD} -ge 8 ] || die "WIFI_PASSWORD must be at least 8 characters"
[ -n "${SSH_KEY:-}" ] || echo "warning: no SSH_KEY; you won't be able to log in to the Pi (the app still works)"

step "Copying base image to $OUT"
case "$BASE" in
  *.xz) command -v xz >/dev/null || die "missing tool: xz"; xz -dcv "$BASE" >"$OUT" ;;
  *) cp --sparse=always "$BASE" "$OUT" ;;
esac

# Partitions are mounted by byte offset: that also works in containers, where
# /dev/loopNpM nodes usually don't appear. 1 = boot (FAT), 2 = root (ext4).
part_start() {
  sfdisk -d "$OUT" | awk -v p="${OUT}$1" '$1 == p && match($0, /start= *[0-9]+/) {
    s = substr($0, RSTART, RLENGTH); gsub(/[^0-9]/, "", s); print s }'
}
ROOT=$(mktemp -d)
cleanup() {
  mountpoint -q "$ROOT" && umount "$ROOT"
  rmdir "$ROOT"
}
trap cleanup EXIT
start=$(part_start 2)
[ -n "$start" ] || die "no second (root) partition in $OUT"
mount -o loop,offset=$((start * 512)) "$OUT" "$ROOT"

[ -e "$ROOT/lib/ld-linux-armhf.so.3" ] || die "not a 32-bit (armhf) Raspberry Pi OS image; plv is built for armv7"
[ -e "$ROOT/usr/lib/arm-linux-gnueabihf/libasound.so.2" ] ||
  echo "warning: libasound2 is missing from the image; plv won't start until it's installed (apt install libasound2)"

step "Binary, service and installer"
install -m 755 "$BIN" "$ROOT/usr/local/bin/plv"
for unit in plv.service plv-wifi-fallback.service plv-wifi-fallback.timer; do
  put_text 644 "$HERE/systemd/$unit" "$ROOT/etc/systemd/system/$unit"
done
put_text 755 "$HERE/systemd/plv-wifi-fallback" "$ROOT/usr/local/lib/plv-wifi-fallback"
# Enabling a unit is just these symlinks; no systemctl (or ARM chroot) needed.
install -d "$ROOT/etc/systemd/system/multi-user.target.wants" "$ROOT/etc/systemd/system/timers.target.wants"
ln -sf /etc/systemd/system/plv.service "$ROOT/etc/systemd/system/multi-user.target.wants/plv.service"
ln -sf /etc/systemd/system/plv-wifi-fallback.timer "$ROOT/etc/systemd/system/timers.target.wants/plv-wifi-fallback.timer"
# install.sh and its files, for re-provisioning on the Pi later (plv-install.sh).
install -d "$ROOT/usr/local/share/plv/systemd"
put_text 755 "$HERE/install.sh" "$ROOT/usr/local/share/plv/install.sh"
for f in plv.service plv-wifi-fallback.service plv-wifi-fallback.timer; do
  put_text 644 "$HERE/systemd/$f" "$ROOT/usr/local/share/plv/systemd/$f"
done
put_text 755 "$HERE/systemd/plv-wifi-fallback" "$ROOT/usr/local/share/plv/systemd/plv-wifi-fallback"
ln -sf /usr/local/share/plv/install.sh "$ROOT/usr/local/bin/plv-install.sh"

step "Song library"
install -d -m 755 "$ROOT/var/lib/plv/songs"
if [ -n "${SONGS_DIR:-}" ]; then
  cp -r "$SONGS_DIR"/. "$ROOT/var/lib/plv/songs/"
  echo "$(find "$ROOT/var/lib/plv/songs" -type f | wc -l) songs copied"
fi

step "Wi-Fi: power save off, fallback hotspot $HOTSPOT"
# A NetworkManager default, so it also covers the Wi-Fi set up in Raspberry Pi Imager.
install -d "$ROOT/etc/NetworkManager/conf.d" "$ROOT/etc/NetworkManager/system-connections"
cat >"$ROOT/etc/NetworkManager/conf.d/plv-wifi-powersave.conf" <<'EOF'
# plv: Wi-Fi power save delays incoming network MIDI by seconds.
[connection]
wifi.powersave = 2
EOF
# pipefail off here: head closing the pipe early is expected (SIGPIPE to tr).
PASS=${HOTSPOT_PASSWORD:-$(set +o pipefail; tr -dc 'a-km-np-z2-9' </dev/urandom | head -c 10)}
[ ${#PASS} -ge 8 ] || die "HOTSPOT_PASSWORD must be at least 8 characters"
# Lower priority than any home network: only used when none is in range.
install -m 600 /dev/null "$ROOT/etc/NetworkManager/system-connections/$HOTSPOT.nmconnection"
cat >"$ROOT/etc/NetworkManager/system-connections/$HOTSPOT.nmconnection" <<EOF
[connection]
id=$HOTSPOT
type=wifi
interface-name=wlan0
autoconnect=true
autoconnect-priority=-10

[wifi]
mode=ap
band=bg
ssid=$HOTSPOT

[wifi-security]
key-mgmt=wpa-psk
psk=$PASS

[ipv4]
method=shared

[ipv6]
method=ignore
EOF
if [ -n "${WIFI_SSID:-}" ]; then
  install -m 600 /dev/null "$ROOT/etc/NetworkManager/system-connections/home.nmconnection"
  cat >"$ROOT/etc/NetworkManager/system-connections/home.nmconnection" <<EOF
[connection]
id=$WIFI_SSID
type=wifi
autoconnect=true

[wifi]
mode=infrastructure
ssid=$WIFI_SSID

[wifi-security]
key-mgmt=wpa-psk
psk=$WIFI_PASSWORD

[ipv4]
method=auto

[ipv6]
method=auto
EOF
fi
install -m 600 /dev/null "$ROOT/var/lib/plv/hotspot-password"
echo "$PASS" >"$ROOT/var/lib/plv/hotspot-password"
echo "$PASS" >"$OUT.hotspot-password.txt"

if [ -n "${STATUS_LOG:-}" ]; then
  step "Status log: plv-status.txt on the boot partition every 2 minutes"
  # Troubleshooting a headless Pi: Windows/macOS can read the boot partition.
  cat >"$ROOT/usr/local/lib/plv-status" <<'EOF'
#!/bin/sh
{
  date; uptime
  echo "== rfkill"; rfkill
  echo "== nmcli general / radio"; nmcli general; nmcli radio
  echo "== devices"; nmcli device
  echo "== connections"; nmcli -f NAME,TYPE,AUTOCONNECT,AUTOCONNECT-PRIORITY,ACTIVE,DEVICE connection
  echo "== iw"; iw dev
  echo "== plv"; systemctl --no-pager status plv plv-wifi-fallback.timer cloud-final ssh 2>&1 | grep -E '●|Active:'
  echo "== failed"; systemctl --failed --no-legend
  echo "== NetworkManager log"; journalctl -b -u NetworkManager --no-pager -n 80
  echo "== wifi kernel log"; journalctl -b -k --no-pager | grep -iE 'brcmf|wlan|cfg80211|regdom' | tail -30
  echo "== plv log"; journalctl -b -u plv --no-pager -n 20
} >/boot/firmware/plv-status.txt 2>&1
EOF
  chmod 755 "$ROOT/usr/local/lib/plv-status"
  cat >"$ROOT/etc/systemd/system/plv-status.service" <<'EOF'
[Unit]
Description=Write plv-status.txt to the boot partition

[Service]
Type=oneshot
ExecStart=/usr/local/lib/plv-status
EOF
  cat >"$ROOT/etc/systemd/system/plv-status.timer" <<'EOF'
[Unit]
Description=Write plv-status.txt every 2 minutes

[Timer]
OnBootSec=1min
OnUnitActiveSec=2min

[Install]
WantedBy=timers.target
EOF
  ln -sf /etc/systemd/system/plv-status.timer "$ROOT/etc/systemd/system/timers.target.wants/plv-status.timer"
fi

sync
umount "$ROOT"

step "First boot: user $PI_USER, hostname $PI_HOSTNAME, Wi-Fi country $WIFI_COUNTRY"
start=$(part_start 1)
[ -n "$start" ] || die "no first (boot) partition in $OUT"
mount -o loop,offset=$((start * 512)) "$OUT" "$ROOT"
[ -f "$ROOT/cmdline.txt" ] || die "boot partition has no cmdline.txt"
# What Raspberry Pi Imager would write; read by cloud-init on first boot.
{
  echo "#cloud-config"
  echo "hostname: $PI_HOSTNAME"
  echo "manage_etc_hosts: true"
  if [ -n "${SSH_KEY:-}" ]; then
    echo "users:"
    echo "- name: $PI_USER"
    echo "  groups: users,adm,dialout,audio,netdev,video,plugdev,cdrom,games,input,gpio,spi,i2c,render,sudo"
    echo "  shell: /bin/bash"
    echo "  lock_passwd: true"
    echo "  sudo: ALL=(ALL) NOPASSWD:ALL"
    echo "  ssh_authorized_keys:"
    echo "  - $SSH_KEY"
    echo "ssh_pwauth: false"
  fi
  echo "runcmd:"
  echo "- [rfkill, unblock, wifi]"
  # Raspberry Pi OS ships with NetworkManager's Wi-Fi switched off; Imager turns it on.
  echo "- [nmcli, radio, wifi, \"on\"]"  # unquoted, YAML reads on as true
  if [ -n "${SSH_KEY:-}" ]; then echo "- [systemctl, enable, --now, ssh]"; fi
} >"$ROOT/user-data"
# The Wi-Fi regulatory domain, as Imager sets it.
sed -i "1s/ *cfg80211.ieee80211_regdom=[A-Z]*//; 1s/\$/ cfg80211.ieee80211_regdom=$WIFI_COUNTRY/" "$ROOT/cmdline.txt"
sync
umount "$ROOT"
echo
echo "Done: $OUT ($(du -h --apparent-size "$OUT" | cut -f1))"
echo "Hotspot $HOTSPOT password: $PASS (also in $OUT.hotspot-password.txt)"
echo "Flash it with Raspberry Pi Imager (Use custom); no customisation needed. First boot takes a few minutes."
