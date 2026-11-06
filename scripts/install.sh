#!/usr/bin/env bash
# ==============================================================================
# Atlantis Pi Home Node: Production Bootstrap & TUI Auto-Launcher
# Targets: Raspberry Pi (aarch64 / ARM64) & Linux nodes anywhere in the world
# ==============================================================================
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${CYAN}${BOLD}"
cat << "EOF"
    ___  ________   ___   _   ___________ _____  ______ _____ 
   / _ \/__   __/  / _ \ / | / /_  __/  _// ___/ / __  //  _/ 
  / /_\ \ / / /   / /_\ //  |/ / / /  / /  \__ \ / /_/ / / /  
 / /___\// / /   / /___\// /| / / / _/ /  ___/ // ____/_/ /   
/_/     /_/ /___/_/     /_/ |_|/_/ /___/ /____//_/    /___/   
EOF
echo -e "${NC}"
echo -e "${GREEN}${BOLD}>> Atlantis Tier 3 Sovereign Pi Node Production Bootstrap${NC}"
echo -e "${CYAN}>> Double-Entry Tide Ledger • OpenRaft Rail • Ratatui TUI (°Fahrenheit)${NC}\n"

# Verify Root Privileges
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}[ERROR] Please run with sudo: curl -sSL <URL> | sudo bash${NC}"
    exit 1
fi

# Detect Architecture
ARCH=$(uname -m)
if [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "x86_64" ]; then
    echo -e "${RED}[ERROR] Unsupported CPU architecture: $ARCH. Atlantis Pi requires aarch64 (ARM64) or x86_64.${NC}"
    exit 1
fi
echo -e "✓ Architecture detected: ${YELLOW}${ARCH}${NC}"

# Setup System Paths & User
ATLANTIS_USER="atlantis"
INSTALL_BIN="/usr/local/bin/atlantis-node"
CONFIG_DIR="/etc/atlantis"
DATA_DIR="/var/lib/atlantis/data"

if ! id -u "$ATLANTIS_USER" >/dev/null 2>&1; then
    echo -e ">> Creating isolated system user: ${ATLANTIS_USER}..."
    useradd --system --no-create-home --shell /usr/sbin/nologin "$ATLANTIS_USER"
fi

mkdir -p "$CONFIG_DIR" "$DATA_DIR"
chown -R "$ATLANTIS_USER:$ATLANTIS_USER" "$DATA_DIR"

# Download Release Binary (GitHub / Production CDN)
RELEASE_URL="${ATLANTIS_RELEASE_URL:-https://github.com/Neptune537/atlantis-node/releases/latest/download/atlantis-node-${ARCH}}"
echo -e ">> Fetching latest production binary for ${ARCH}..."
if curl -fsSL -o "$INSTALL_BIN" "$RELEASE_URL" 2>/dev/null; then
    chmod +x "$INSTALL_BIN"
    echo -e "✓ Installed binary to ${GREEN}${INSTALL_BIN}${NC}"
else
    echo -e "${YELLOW}[NOTICE] Remote pre-built binary CDN unreachable or private repository.${NC}"
    echo -e ">> Checking for local Docker runtime fallback..."
    if command -v docker >/dev/null 2>&1; then
        echo -e "✓ Docker detected. Launching via official container image..."
        docker run -d \
            --name atlantis-home-node \
            --restart unless-stopped \
            -p 8081:8081 \
            -p 9001:9001 \
            -v /sys/class/thermal:/sys/class/thermal:ro \
            -v "$DATA_DIR":/app/data \
            -e ATLANTIS_NODE_ID="$(hostname)-pi" \
            -e ATLANTIS_DIVIDEND_RATE=0.02 \
            ghcr.io/neptune537/atlantis-home-node:latest || true
    fi
fi

# Create symlink for standard CLI access
ln -sf "$INSTALL_BIN" /usr/local/bin/atlantis

# Create Production Systemd Service for Headless Auto-Restart
SYSTEMD_PATH="/etc/systemd/system/atlantis.service"
echo -e ">> Installing production systemd service..."
cat << EOF > "$SYSTEMD_PATH"
[Unit]
Description=Atlantis Club Home Node (Double-Entry Ledger & Consensus Rail)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${ATLANTIS_USER}
Group=${ATLANTIS_USER}
WorkingDirectory=/var/lib/atlantis
ExecStart=${INSTALL_BIN} --headless
Restart=always
RestartSec=5s
Environment=ATLANTIS_NODE_ID=$(hostname)-pi
Environment=ATLANTIS_DIVIDEND_RATE=0.02
Environment=RUST_LOG=info
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable atlantis.service
systemctl restart atlantis.service
echo -e "✓ Background daemon running under systemd (${GREEN}atlantis.service${NC})"

echo -e "\n${GREEN}${BOLD}✓ Atlantis Pi Node successfully installed!${NC}"
echo -e "API endpoint: http://localhost:8081"
echo -e "Consensus WebSocket: ws://localhost:9001"
echo -e "To view the dashboard at any time, run: ${CYAN}${BOLD}atlantis${NC}\n"

# If attached to an interactive terminal, launch the Ratatui TUI immediately
if [ -t 0 ] && [ -x "$INSTALL_BIN" ]; then
    echo -e "${CYAN}>> Launching Atlantis American Metrics Dashboard (Ratatui TUI)...${NC}"
    sleep 1
    exec "$INSTALL_BIN"
fi
