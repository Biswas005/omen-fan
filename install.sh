#!/bin/bash

set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root. Use sudo."
   exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DST_DIR="/usr/local/bin"
DESKTOP_DST="/usr/share/applications/omen-ui.desktop"
SERVICE_SRC="$SCRIPT_DIR/packaging/systemd/omen-fand.service"
SERVICE_DST="/etc/systemd/system/omen-daemon.service"

install_binary() {
    local name="$1"
    local src="$SCRIPT_DIR/$name"

    if [[ ! -f "$src" ]]; then
        src="$SCRIPT_DIR/target/release/$name"
    fi

    if [[ ! -f "$src" ]]; then
        printf 'Error: binary \u0027%s\u0027 not found. Checked:\n  %s/%s\n  %s/target/release/%s\n' "$name" "$SCRIPT_DIR" "$name" "$SCRIPT_DIR" "$name"
        exit 1
    fi

    cp "$src" "$BIN_DST_DIR/$name"
    chmod +x "$BIN_DST_DIR/$name"
    echo "Installed: $BIN_DST_DIR/$name"
}

for name in omen-cli omen-daemon omen-ui; do
    install_binary "$name"
done

mkdir -p "$(dirname "$DESKTOP_DST")"
cat > "$DESKTOP_DST" <<'DESKTOP_EOF'
[Desktop Entry]
Name=OMEN Fan Control
Comment=Open OMEN fan control UI
Exec=/usr/local/bin/omen-ui
Icon=utilities-system-monitor
Terminal=false
Type=Application
Categories=Utility;System;
StartupWMClass=omen-ui
DESKTOP_EOF

chmod 644 "$DESKTOP_DST"
echo "Installed desktop entry: $DESKTOP_DST"

if [[ -f "$SERVICE_SRC" ]]; then
    sed "s|ExecStart=.*|ExecStart=$BIN_DST_DIR/omen-daemon|" "$SERVICE_SRC" > "$SERVICE_DST"
    chmod 644 "$SERVICE_DST"
    systemctl daemon-reload
    systemctl enable omen-daemon.service
    systemctl restart omen-daemon.service
    echo "Installed and enabled systemd service: omen-daemon.service"
else
    echo "Warning: service file not found at $SERVICE_SRC. Skipping systemd service install."
fi

echo "Installation complete. omen-cli, omen-daemon and omen-ui are installed."
