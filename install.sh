#!/bin/bash

set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root. Use sudo."
   exit 1
fi

# Variables
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="$SCRIPT_DIR/omen-fan"
BIN_DST="/usr/local/bin/omen-fan"
TOML_SRC="$SCRIPT_DIR/fan_config.toml"
TOML_DST="/usr/local/bin/fan_config.toml"
SERVICE_SRC="$SCRIPT_DIR/omen-fan.service"
SERVICE_DST="/etc/systemd/system/omen-fan.service"

# Copy binary and config
cp "$BIN_SRC" "$BIN_DST"
chmod +x "$BIN_DST"

cp "$TOML_SRC" "$TOML_DST"
chmod 777 "$TOML_DST"

# Replace config path in the service file
sed "s|--config [^ ]*|--config $TOML_DST|g; s|ExecStart=[^ ]*|ExecStart=$BIN_DST --config $TOML_DST|g" "$SERVICE_SRC" > "$SERVICE_DST"

chmod 644 "$SERVICE_DST"
systemctl daemon-reload
systemctl enable omen-fan.service
systemctl restart omen-fan.service

echo "Installation complete. omen-fan service is running."