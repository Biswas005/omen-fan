#!/bin/bash

set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root. Use sudo."
   exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DESKTOP_DST="/usr/share/applications/omen-ui.desktop"
SERVICE_SRC="$SCRIPT_DIR/packaging/systemd/omen-daemon.service"

if [[ -f "/run/ostree-booted" ]]; then
    # Silverblue / immutable distros
    BIN_DST_DIR="/usr/local/bin"
    SERVICE_DST="/etc/systemd/system/omen-daemon.service"
else
    # Normal distros
    BIN_DST_DIR="/usr/bin"
    SERVICE_DST="/usr/lib/systemd/system/omen-daemon.service"
fi

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

    mkdir -p "$BIN_DST_DIR"
    cp "$src" "$BIN_DST_DIR/$name"
    chmod +x "$BIN_DST_DIR/$name"
    echo "Installed: $BIN_DST_DIR/$name"
}

for name in omen-cli omen-daemon omen-ui; do
    install_binary "$name"
done

mkdir -p "$(dirname "$DESKTOP_DST")"
cat > "$DESKTOP_DST" <<DESKTOP_EOF
[Desktop Entry]
Name=OMEN Fan Control
Comment=Open OMEN fan control UI
Exec=$BIN_DST_DIR/omen-ui
Icon=utilities-system-monitor
Terminal=false
Type=Application
Categories=Utility;System;
StartupWMClass=omen-ui
DESKTOP_EOF

chmod 644 "$DESKTOP_DST"
echo "Installed desktop entry: $DESKTOP_DST"

if [[ -f "$SERVICE_SRC" ]]; then
    mkdir -p "$(dirname "$SERVICE_DST")"
    sed "s|ExecStart=.*|ExecStart=$BIN_DST_DIR/omen-daemon|" "$SERVICE_SRC" > "$SERVICE_DST"
    chmod 644 "$SERVICE_DST"
    systemctl daemon-reload
    systemctl enable omen-daemon.service
    systemctl restart omen-daemon.service
    echo "Installed and enabled systemd service: omen-daemon.service"
else
    echo "Warning: service file not found at $SERVICE_SRC. Skipping systemd service install."
fi

echo "Building and installing hp-wmi.ko kernel module..."
cd "$SCRIPT_DIR/Driver"
make -C /lib/modules/$(uname -r)/build M=$PWD modules

echo "Generating MOK key for Secure Boot..."
openssl req -new -x509 -newkey rsa:2048 -keyout MOK.priv -outform DER -out MOK.der -nodes -days 36500 -subj "/CN=OMEN Fan Control/"

echo "Signing hp-wmi.ko..."
SIGN_FILE_SCRIPT="/lib/modules/$(uname -r)/build/scripts/sign-file"
if [[ ! -x "$SIGN_FILE_SCRIPT" ]]; then
    SIGN_FILE_SCRIPT="/usr/src/kernels/$(uname -r)/scripts/sign-file"
fi

if [[ -x "$SIGN_FILE_SCRIPT" ]]; then
    "$SIGN_FILE_SCRIPT" sha256 MOK.priv MOK.der hp-wmi.ko
else
    echo "Warning: sign-file script not found. Skipping module signing."
fi

MODULE_DIR="/lib/modules/$(uname -r)/updates"
mkdir -p "$MODULE_DIR"
cp hp-wmi.ko "$MODULE_DIR/"
depmod -a

echo "Importing MOK key. You will be prompted to enter a one-time password."
echo "Please remember this password, as you will need it to enroll the key on the next boot."
if command -v mokutil >/dev/null 2>&1; then
    mokutil --import MOK.der || echo "Warning: Failed to import MOK key (maybe Secure Boot is disabled)."
else
    echo "Warning: mokutil not found. Ensure Secure Boot is disabled or manually enroll the key."
fi

echo ""
echo "================================================================="
echo "Installation complete!"
echo "omen-cli, omen-daemon and omen-ui are installed."
echo ""
echo "*** IMPORTANT ***"
echo "Please RESTART your system now."
echo "During the next boot, Shim will prompt you to enroll the MOK key."
echo "Choose 'Enroll MOK', continue, and enter the password you just set."
echo "================================================================="
