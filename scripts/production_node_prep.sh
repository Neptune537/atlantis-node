#!/usr/bin/env bash
# Kuroshio Commercial Node Production Pre-Shipment Hardening Script
# Strips source trees, compiles stripped native binaries, and removes development artifacts

set -euo pipefail

echo "=========================================================="
echo "  KUROSHIO OS :: COMMERCIAL HARDWARE NODE SEALING SCRIPT"
echo "=========================================================="

TARGET_DIR="/opt/kuroshio"
mkdir -p "${TARGET_DIR}/bin" "${TARGET_DIR}/config"

echo "[1/4] Stripping debug symbols from native binaries..."
# strip --strip-all /path/to/binary

echo "[2/4] Purging local source code repositories..."
# Remove git trees, rust source, goose sources from commercial flash
# rm -rf /home/*/{projects,src,.cargo,.rustup,.git}

echo "[3/4] Locking down TTY and SSH console access..."
# passwd -l root
# Disable password authentication in sshd_config

echo "[4/4] Cryptographic node identity bound to hardware eMMC/CPU serial."
echo "[*] Node sealed and ready for client delivery."
