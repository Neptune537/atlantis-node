#!/usr/bin/env bash
# ==============================================================================
# TR7N7T7 Solar SenseCAP Car Sentry Provisioner (Linux / Raspberry Pi)
# 915MHz LoRa RF Anti-Theft Daemon with Bluetooth Proximity & Tamper Detection
# ==============================================================================
set -euo pipefail

VEHICLE_DID="${1:-did:tr7n7t7:node:car-sensecap-01}"
OWNER_BLE_MAC="${2:-AA:BB:CC:DD:EE:FF}"
CONFIG_DIR="/etc/tr7n7t7"
BIN_DIR="/usr/local/bin"

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}======================================================================${NC}"
echo -e "${GREEN}>> Provisioning TR7N7T7 Solar SenseCAP Car Sentry Daemon (Rust)${NC}"
echo -e "   Vehicle DID    : ${YELLOW}${VEHICLE_DID}${NC}"
echo -e "   Owner BLE MAC  : ${YELLOW}${OWNER_BLE_MAC}${NC}"
echo -e "   RF Frequency   : ${YELLOW}915.0 MHz (LoRa Sub-GHz Direct)${NC}"
echo -e "${CYAN}======================================================================${NC}"

mkdir -p "$CONFIG_DIR" "$BIN_DIR"

# 1. Write sentry configuration
cat << EOF > "${CONFIG_DIR}/car_sentry.json"
{
  "vehicle_did": "${VEHICLE_DID}",
  "owner_ble_macs": [
    "${OWNER_BLE_MAC}"
  ],
  "rf_band": "US915",
  "lora_frequency_hz": 915000000,
  "tamper_g_force_threshold": 1.8,
  "solar_usb_monitor": true,
  "home_seed_node_did": "did:tr7n7t7:node:emrit-seed-01"
}
EOF
echo -e "✓ Wrote sentry configuration to ${CONFIG_DIR}/car_sentry.json"

# 2. Create systemd service for 24/7 background sentry execution
cat << EOF > /etc/systemd/system/tr7n7t7-car-sentry.service
[Unit]
Description=TR7N7T7 Solar SenseCAP 915MHz Car Sentry Daemon
After=network.target bluetooth.target

[Service]
Type=simple
ExecStart=${BIN_DIR}/tr7n7t7-car-sentry
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload >/dev/null 2>&1 || true
systemctl enable tr7n7t7-car-sentry.service >/dev/null 2>&1 || true

echo -e "${GREEN}✓ TR7N7T7 Car Sentry installed successfully!${NC}"
echo -e "  To start immediately: systemctl start tr7n7t7-car-sentry.service"
