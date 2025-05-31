#!/bin/bash

set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root. Use sudo."
   exit 1
fi

# Variables
REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="$REPO_DIR/target/release/omen-fan"
BIN_DST="/usr/local/bin/omen-fan"
TOML_SRC="$REPO_DIR/src/fan_config.toml"
TOML_DST="/usr/local/bin/fan_config.toml"
SERVICE_SRC="$REPO_DIR/src/omen-fan.service"
SERVICE_DST="/etc/systemd/system/omen-fan.service"

# Build the binary
cd "$REPO_DIR"
cargo build --release

# Copy binary and config
cp "$BIN_SRC" "$BIN_DST"
chmod +x "$BIN_DST"

cp "$TOML_SRC" "$TOML_DST"
chmod 777 "$TOML_DST"

# Automatically replace any repo path in the service file
# Find the config path in the service file and replace with TOML_DST
sed "s|--config [^ ]*|--config $TOML_DST|g; s|ExecStart=[^ ]*|ExecStart=$BIN_DST --config $TOML_DST|g" "$SERVICE_SRC" > "$SERVICE_DST"

chmod 644 "$SERVICE_DST"
systemctl daemon-reload
systemctl enable omen-fan.service
systemctl restart omen-fan.service

echo "Installation complete. omen-fan service is running."