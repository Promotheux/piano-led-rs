#!/usr/bin/env bash
# Installs plv on a Raspberry Pi (Raspberry Pi OS Bookworm or later, NetworkManager).
#
#   sudo ./install.sh [path/to/plv]      # default binary: ./plv
#   sudo ./install.sh --dry-run          # show every step, change nothing
#
# Safe to run again (e.g. to update the binary). Environment:
#   HOTSPOT_PASSWORD   password for the fallback hotspot (default: random, printed)
set -euo pipefail

DRY_RUN=
BIN=./plv
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) BIN=$arg ;;
  esac
done

HERE=$(cd "$(dirname "$0")" && pwd)
STATE=/var/lib/plv
ORIGINAL_SONGS=/home/Piano-LED-Visualizer/Songs
HOTSPOT=PianoLED-Setup

run() {
  echo "+ $*"
  [ -n "$DRY_RUN" ] || "$@"
}

# Writes stdin to a file (or shows where it would go in a dry run).
put() {
  if [ -n "$DRY_RUN" ]; then echo "+ write $1"; cat >/dev/null; else cat >"$1"; fi
}

step() { echo; echo "== $*"; }

[ -n "$DRY_RUN" ] || [ "$(id -u)" -eq 0 ] || { echo "Run as root: sudo $0 $*" >&2; exit 1; }
[ -f "$BIN" ] || { echo "No plv binary at '$BIN'. Build it (see README, Build) and pass its path." >&2; exit 1; }
[ -f "$HERE/systemd/plv.service" ] || { echo "Run this from a checkout of the repo (systemd/plv.service missing)." >&2; exit 1; }

step "Binary and service"
run install -m 755 "$BIN" /usr/local/bin/plv
run install -m 644 "$HERE/systemd/plv.service" /etc/systemd/system/plv.service
run install -d -m 755 "$STATE"

step "Song library"
if [ -e "$STATE/songs" ]; then
  echo "keeping $STATE/songs"
elif [ -d "$ORIGINAL_SONGS" ]; then
  # Share the original visualizer's library instead of copying it.
  run ln -s "$ORIGINAL_SONGS" "$STATE/songs"
else
  run install -d -m 755 "$STATE/songs"
fi

step "Original Python visualizer"
if systemctl cat visualizer.service >/dev/null 2>&1; then
  # Both drive GPIO18 and port 80. Its files stay, so it can be re-enabled.
  run systemctl disable --now visualizer.service
else
  echo "not installed"
fi

step "rtpmidid (replaced by plv's built-in network MIDI on the same port)"
if systemctl cat rtpmidid.service >/dev/null 2>&1; then
  run systemctl disable --now rtpmidid.service
else
  echo "not installed"
fi

step "Wi-Fi power save (off: it delays incoming network MIDI by seconds)"
# Every client Wi-Fi profile (not hotspots), so it stays off whichever one the Pi joins.
nmcli -t -f NAME,TYPE connection show | awk -F: '$2=="802-11-wireless" {print $1}' | while IFS= read -r wifi; do
  [ "$(nmcli -g 802-11-wireless.mode connection show "$wifi")" = ap ] && continue
  run nmcli connection modify "$wifi" 802-11-wireless.powersave 2
done
# Profiles apply at the next reconnect; turn it off right away too.
if [ -x /sbin/iw ]; then run /sbin/iw dev wlan0 set power_save off; fi

step "Fallback hotspot ($HOTSPOT)"
if nmcli -t -f NAME connection show | grep -qx "$HOTSPOT"; then
  echo "already configured"
else
  # pipefail off here: head closing the pipe early is expected (SIGPIPE to tr).
  PASS=${HOTSPOT_PASSWORD:-$(set +o pipefail; tr -dc 'a-km-np-z2-9' </dev/urandom | head -c 10)}
  # Lower priority than the home network: NetworkManager only falls back to it
  # when no known network is in range.
  run nmcli connection add type wifi ifname wlan0 con-name "$HOTSPOT" ssid "$HOTSPOT" \
    802-11-wireless.mode ap 802-11-wireless.band bg ipv4.method shared \
    wifi-sec.key-mgmt wpa-psk wifi-sec.psk "$PASS" \
    connection.autoconnect yes connection.autoconnect-priority -10
  [ -n "$DRY_RUN" ] || { echo "$PASS" >"$STATE/hotspot-password"; chmod 600 "$STATE/hotspot-password"; }
  echo "hotspot password: $PASS (saved in $STATE/hotspot-password)"
fi

# On the hotspot the Pi can't see whether home Wi-Fi is back. Every 2 minutes,
# if nobody is connected to it, step down so NetworkManager tries home again.
put /usr/local/lib/plv-wifi-fallback <<EOF
#!/bin/sh
nmcli -t -f NAME connection show --active | grep -qx "$HOTSPOT" || exit 0
[ "\$(/sbin/iw dev wlan0 station dump | grep -c '^Station')" -eq 0 ] && nmcli connection down "$HOTSPOT"
exit 0
EOF
run chmod 755 /usr/local/lib/plv-wifi-fallback
put /etc/systemd/system/plv-wifi-fallback.service <<'EOF'
[Unit]
Description=Leave the plv fallback hotspot when nobody uses it

[Service]
Type=oneshot
ExecStart=/usr/local/lib/plv-wifi-fallback
EOF
put /etc/systemd/system/plv-wifi-fallback.timer <<'EOF'
[Unit]
Description=Check every 2 minutes whether home Wi-Fi is back

[Timer]
OnBootSec=3min
OnUnitActiveSec=2min

[Install]
WantedBy=timers.target
EOF

step "Start"
run systemctl daemon-reload
run systemctl enable --now plv-wifi-fallback.timer
run systemctl enable plv.service
run systemctl restart plv.service

IP=$(hostname -I | awk '{print $1}')
echo
echo "Done. Open http://$IP/ (or https://$IP/ to install the app on Android)."
echo "Without home Wi-Fi: join '$HOTSPOT' and open http://10.42.0.1/"
