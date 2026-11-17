#!/usr/bin/env bash
# Kuroshio Autonomous Pi Node Firewall & LAN Hardening Script
# Blocks public internet probing while allowing local mesh (10.0.0.0/24) and Cloudflare Tunnel

set -euo pipefail

echo "=========================================================="
echo "  KUROSHIO OS :: NODE FIREWALL & LAN HARDENING (UFW/IPTABLES)"
echo "=========================================================="

if ! command -v ufw >/dev/null 2>&1; then
    echo "[!] UFW not installed, setting up iptables rules..."
    # Drop all incoming from non-LAN interfaces by default
    iptables -P INPUT DROP
    iptables -P FORWARD DROP
    iptables -P OUTPUT ACCEPT

    # Allow loopback
    iptables -A INPUT -i lo -j ACCEPT

    # Allow established connections
    iptables -A INPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT

    # Allow LAN Subnet (10.0.0.0/24) for Atlantis Mesh & Raft
    iptables -A INPUT -s 10.0.0.0/24 -p tcp -m multiport --dports 22,8081,8082,8787,9001 -j ACCEPT

    echo "[*] Direct iptables hardening rules applied successfully."
    exit 0
fi

# Reset UFW to secure defaults
ufw --force reset
ufw default deny incoming
ufw default allow outgoing

# Allow Loopback
ufw allow in on lo

# Restrict critical ports strictly to the trusted LAN subnet (10.0.0.0/24)
ufw allow from 10.0.0.0/24 to any port 22 proto tcp comment 'SSH Admin (Local LAN Only)'
ufw allow from 10.0.0.0/24 to any port 8081 proto tcp comment 'Atlantis Mesh API (Local LAN Only)'
ufw allow from 10.0.0.0/24 to any port 8082 proto tcp comment 'Siren Radio Local Ingest'
ufw allow from 10.0.0.0/24 to any port 8787 proto tcp comment 'Charter Cyan Engine (Local LAN Only)'
ufw allow from 10.0.0.0/24 to any port 9001 proto tcp comment 'OpenRaft Consensus (Local LAN Only)'

# Enable Firewall
ufw --force enable

echo "[*] UFW successfully hardened! Public port scanning blocked; local mesh open."
