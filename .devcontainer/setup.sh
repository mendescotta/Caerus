#!/bin/sh
# Installs Caerus's build and check dependencies in a Void Linux dev container.
# Run by the codespace configurations as their postCreateCommand:
#   sh .devcontainer/setup.sh            build/check tools only
#   sh .devcontainer/setup.sh --desktop  also Xvfb, x11vnc and noVNC
set -eu

NOVNC_VERSION=1.7.0
NOVNC_SHA256=b1003a11b6e6e8d8f7f5e5586daae7f8ca651d8aee0aa155ff9ac841c48f52c6

xbps-install -Sy
xbps-install -uy xbps
xbps-install -Suy
xbps-install -y base-devel libxbps-devel gtk4-devel libadwaita-devel pkg-config \
	rust cargo rust-analyzer clang zstd git github-cli python3 uv shellcheck ripgrep

if [ "${1-}" = --desktop ]; then
	xbps-install -y xorg-server-xvfb x11vnc openbox dbus adwaita-icon-theme \
		dejavu-fonts-ttf curl
	tmp=$(mktemp)
	trap 'rm -f "$tmp"' EXIT
	curl -fsSL -o "$tmp" \
		"https://github.com/novnc/noVNC/archive/refs/tags/v$NOVNC_VERSION.tar.gz"
	echo "$NOVNC_SHA256  $tmp" | sha256sum -c -
	rm -rf /opt/novnc
	mkdir -p /opt/novnc
	tar -xzf "$tmp" -C /opt/novnc --strip-components=1
	ln -sf vnc.html /opt/novnc/index.html
	# websockify and its dependencies come from a hash-locked requirements file.
	uv venv -q --allow-existing --python /usr/bin/python3 /opt/websockify
	uv pip install -q --python /opt/websockify --require-hashes \
		-r "$(dirname "$0")/desktop/websockify-requirements.txt"
	ln -sf /opt/websockify/bin/websockify /usr/local/bin/websockify
fi
