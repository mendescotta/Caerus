#!/bin/sh
# Starts a virtual X display with a browser-viewable VNC session on port 6080.
# Run automatically on codespace start; run again by hand if a process exits.
# Then launch the GUI from the terminal: cargo run -p caerus
set -eu

display=${DISPLAY:-:1}
logdir=/tmp/caerus-desktop
mkdir -p "$logdir"

running() { pgrep -x "$1" >/dev/null 2>&1; }

running Xvfb || setsid Xvfb "$display" -screen 0 1440x900x24 -nolisten tcp \
	>"$logdir/xvfb.log" 2>&1 &
i=0
until [ -S "/tmp/.X11-unix/X${display#:}" ]; do
	i=$((i + 1))
	[ "$i" -lt 50 ] || { echo "Xvfb did not start; see $logdir/xvfb.log" >&2; exit 1; }
	sleep 0.1
done

running openbox || DISPLAY=$display setsid openbox >"$logdir/openbox.log" 2>&1 &
running x11vnc || setsid x11vnc -display "$display" -localhost -forever -shared \
	-nopw -rfbport 5900 >"$logdir/x11vnc.log" 2>&1 &
running websockify || setsid websockify --web /opt/novnc 127.0.0.1:6080 localhost:5900 \
	>"$logdir/websockify.log" 2>&1 &

# Fail loudly (with the log to read) if a service did not come up, for example
# because its port is already taken. A port is listening when /proc/net/tcp has
# it in state 0A; the process must also still be running, since another program
# could hold the port.
wait_for() {
	port=$(printf '%04X' "$2")
	i=0
	until awk -v p=":$port" '$4 == "0A" && substr($2, length($2) - 4) == p { f = 1 } END { exit !f }' \
		/proc/net/tcp; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || { echo "$1 is not listening on port $2; see $logdir/$1.log" >&2; exit 1; }
		sleep 0.1
	done
	sleep 0.5
	running "$1" || { echo "$1 exited; see $logdir/$1.log" >&2; exit 1; }
}
wait_for x11vnc 5900
wait_for websockify 6080

echo "Desktop ready on port 6080. Start Caerus with: cargo run -p caerus"
