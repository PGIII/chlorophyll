#!/bin/bash
# Install the chlorophyll sensor_server LaunchAgent into the GUI (Aqua) session.
# Run this on the mini, after `cargo build --release -p sensor_server`.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
LA="$HOME/Library/LaunchAgents"
uid="$(id -u)"
label="com.chlorophyll"

if [ ! -x "$ROOT/target/release/sensor_server" ]; then
    echo "missing $ROOT/target/release/sensor_server — run: cargo build --release -p sensor_server" >&2
    exit 1
fi

mkdir -p "$LA" "$ROOT/data"

launchctl bootout "gui/$uid/$label" 2>/dev/null || true
launchctl unload "$LA/$label.plist" 2>/dev/null || true

cp "$HERE/$label.plist" "$LA/$label.plist"
if launchctl bootstrap "gui/$uid" "$LA/$label.plist" 2>/dev/null; then
    launchctl kickstart -k "gui/$uid/$label" 2>/dev/null || true
    echo "bootstrapped $label into gui/$uid"
else
    launchctl load -w "$LA/$label.plist"
    echo "loaded $label via load -w (fallback)"
fi

sleep 2
launchctl list | grep -i chlorophyll || echo "(not in launchctl list yet)"
echo "logs: data/server.log  data/server.err"
echo "dashboard: http://$(scutil --get LocalHostName).local:5001"
