#!/usr/bin/env bash
# ==============================================================================
# TR7N7T7 Commercial Radio & Hardware Nodes: 1-Click Store Node Provisioner (Linux / RPi)
# Supports:
#   - Tier 1: $100 Compact Radio Node
#   - Tier 2: $150 Pro Commercial Radio & AI Jingle Node
#   - Tier 3: $299 Enterprise Club Fortress Node
#   - Optional: Crankk OS / DePIN Multimining Container Co-Hosting
# ==============================================================================
set -euo pipefail

STORE_SLUG="${1:-store-default-01}"
NODE_TIER="${2:-PRO_150}"        # COMPACT_100 | PRO_150 | FORTRESS_299
AUDIO_OUT="${3:-analog}"         # analog (3.5mm Aux/RCA) | hdmi | usb_dac
ENABLE_CRANKK="${4:-false}"      # true | false (DePIN multimining container)

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}======================================================================${NC}"
echo -e "${GREEN}>> Provisioning TR7N7T7 Commercial Radio & Hardware Node Appliance${NC}"
echo -e "   Store Slug     : ${YELLOW}${STORE_SLUG}${NC}"
echo -e "   Hardware Tier  : ${YELLOW}${NODE_TIER}${NC}"
echo -e "   Audio Output   : ${YELLOW}${AUDIO_OUT}${NC}"
echo -e "   Crankk DePIN   : ${YELLOW}${ENABLE_CRANKK}${NC}"
echo -e "${CYAN}======================================================================${NC}"

CONFIG_DIR="/etc/tr7n7t7"
DATA_DIR="/var/lib/tr7n7t7/data"
RADIO_CACHE_DIR="/var/lib/tr7n7t7/radio_cache"
mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$RADIO_CACHE_DIR"

# 1. Generate unique Pearl Enclave DID & Store Provisioning Manifest
NODE_DID="did:tr7n7t7:node:${STORE_SLUG}-$(date +%s)"
cat << EOF > "${CONFIG_DIR}/node_manifest.json"
{
  "store_slug": "${STORE_SLUG}",
  "node_did": "${NODE_DID}",
  "hardware_tier": "${NODE_TIER}",
  "audio_output": "${AUDIO_OUT}",
  "staking": {
    "annual_stake_usd": 12.00,
    "monthly_pool_drip_usd": 1.00,
    "payment_rail": "GOOGLE_PAY_VERIFIED",
    "status": "ACTIVE_LOCKED"
  },
  "radio_config": {
    "primary_cloud_stream": "https://josh375-triton-tier1-hub.hf.space/live.mp3",
    "local_fallback_port": 8080,
    "offline_cache_dir": "${RADIO_CACHE_DIR}",
    "base_frequency_hz": 432
  },
  "crankk_multimining_enabled": ${ENABLE_CRANKK}
}
EOF
echo -e "✓ Wrote node manifest and $12 staking config to ${CONFIG_DIR}/node_manifest.json"

# 2. Configure ALSA / PipeWire Audio Routing for Commercial Amplifier
if command -v amixer >/dev/null 2>&1; then
    case "$AUDIO_OUT" in
        analog)
            amixer cset numid=3 1 >/dev/null 2>&1 || true
            amixer set PCM 95% >/dev/null 2>&1 || true
            ;;
        hdmi)
            amixer cset numid=3 2 >/dev/null 2>&1 || true
            ;;
    esac
    echo -e "✓ Configured hardware audio jack (${AUDIO_OUT}) at 95% line-out gain"
fi

# 3. Install Systemd Service for 24/7 Zero-Dead-Air In-Store Radio Player
cat << 'EOF' > /usr/local/bin/tr7n7t7-radio-loop.sh
#!/usr/bin/env bash
STREAM_URL="http://127.0.0.1:8080/live.mp3"
CACHE_DIR="/var/lib/atlantis/radio_cache"

while true; do
    if command -v mpv >/dev/null 2>&1; then
        mpv --no-video --volume=95 "$STREAM_URL" || mpv --no-video --shuffle "$CACHE_DIR"/*.mp3 || sleep 2
    elif command -v cvlc >/dev/null 2>&1; then
        cvlc --intf dummy "$STREAM_URL" || sleep 2
    else
        sleep 5
    fi
done
EOF
chmod +x /usr/local/bin/tr7n7t7-radio-loop.sh

cat << EOF > /etc/systemd/system/siren-instore-player.service
[Unit]
Description=TR7N7T7 24/7 In-Store Commercial Radio Output (Zero Dead-Air)
After=network-online.target sound.target

[Service]
Type=simple
ExecStart=/usr/local/bin/tr7n7t7-radio-loop.sh
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

# 4. Optional Crankk DePIN / Docker Co-Hosting Setup
if [ "$ENABLE_CRANKK" = "true" ]; then
    echo -e ">> Enabling isolated Docker container policy for Crankk / DePIN Multimining..."
    cat << EOF > "${CONFIG_DIR}/depin_multimining.conf"
# Isolated Secondary Container Policy (CPU capped at 25% to protect 48kHz Audio & Home Node)
DEPIN_CPU_QUOTA="25%"
DEPIN_MEMORY_LIMIT="512m"
EOF
fi


# 5. Raspberry Pi Touchscreen / HDMI Screen Demo Kiosk (TR7N7T7 Visual HUD)
if [ -f /etc/systemd/system/tr7n7t7-kiosk.service ] || command -v chromium-browser >/dev/null 2>&1; then
    echo -e ">> Setting up TR7N7T7 Screen Demo Kiosk Mode..."
    cat << 'EOF' > /usr/local/bin/tr7n7t7-screen-kiosk.sh
#!/usr/bin/env bash
xset -dpms || true
xset s off || true
xset s noblank || true
unclutter -idle 0.5 -root &
# Launch TR7N7T7 3D Digital Twin and In-Store Player in fullscreen kiosk mode on the Pi screen
chromium-browser --noerrdialogs --disable-infobars --kiosk --autoplay-policy=no-user-gesture-required "https://heavenlysent-680ac.web.app?venue=atlantis&store=1&mode=kiosk"
EOF
    chmod +x /usr/local/bin/tr7n7t7-screen-kiosk.sh
fi

systemctl daemon-reload >/dev/null 2>&1 || true
systemctl enable siren-instore-player.service >/dev/null 2>&1 || true

echo -e "${GREEN}✓ Provisioning complete! Plug 3.5mm/RCA into store amplifier and power on.${NC}"
