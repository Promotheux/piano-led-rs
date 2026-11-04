#!/usr/bin/env bash
# Builds a ready-to-flash Raspberry Pi OS image with plv preinstalled.
#
#   sudo ./build_image.sh <raspios-lite-armhf.img[.xz]> [plv-binary] [output.img]
#
# Base: 32-bit Raspberry Pi OS Lite (Bookworm or later), as downloaded; it is
# never modified. Defaults: binary target/armv7-unknown-linux-gnueabihf/release/plv,
# output plv-<date>.img. Environment: HOTSPOT_PASSWORD (default: random),
# SONGS_DIR (songs to preload).
#
# Linux, as root. On Windows/macOS run it in Docker:
#   docker run --rm --privileged -v "$PWD:/src" -w /src plv-armv7 ./build_image.sh <base>
#
# Flash the result with Raspberry Pi Imager and set user, Wi-Fi and SSH in its
# settings. No chroot or ARM emulation: everything is copied in as files.
set -euo pipefail

BASE=${1:?usage: $0 <raspios-lite-armhf.img[.xz]> [plv-binary] [output.img]}
BIN=${2:-target/armv7-unknown-linux-gnueabihf/release/plv}
OUT=${3:-plv-$(date +%Y%m%d).img}
HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
HOTSPOT=PianoLED-Setup

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

step "Copying base image to $OUT"
case "$BASE" in
  *.xz) command -v xz >/dev/null || die "missing tool: xz"; xz -dc "$BASE" >"$OUT" ;;
  *) cp --sparse=always "$BASE" "$OUT" ;;
esac

# The root filesystem is the second partition. Mount it by byte offset: that
# also works in containers, where /dev/loopNpM nodes usually don't appear.
start=$(sfdisk -d "$OUT" | awk -v p="${OUT}2" '$1 == p && match($0, /start= *[0-9]+/) {
  s = substr($0, RSTART, RLENGTH); gsub(/[^0-9]/, "", s); print s }')
[ -n "$start" ] || die "no second (root) partition in $OUT"
ROOT=$(mktemp -d)
cleanup() {
  mountpoint -q "$ROOT" && umount "$ROOT"
  rmdir "$ROOT"
}
trap cleanup EXIT
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
install -m 600 /dev/null "$ROOT/var/lib/plv/hotspot-password"
echo "$PASS" >"$ROOT/var/lib/plv/hotspot-password"
echo "$PASS" >"$OUT.hotspot-password.txt"

sync
umount "$ROOT"
echo
echo "Done: $OUT ($(du -h --apparent-size "$OUT" | cut -f1))"
echo "Hotspot $HOTSPOT password: $PASS (also in $OUT.hotspot-password.txt)"
echo "Flash it with Raspberry Pi Imager (Use custom) and set user, Wi-Fi and SSH in its settings."
