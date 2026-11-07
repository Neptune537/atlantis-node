# Atlantis Node (Tier 3 Epipelagic & Abyssal Node Core)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/Neptune537/atlantis-node)](https://github.com/Neptune537/atlantis-node/releases)
[![Build Status](https://img.shields.io/github/actions/workflow/status/Neptune537/atlantis-node/release-aarch64.yml?branch=master)](https://github.com/Neptune537/atlantis-node/actions)
[![Target: aarch64-unknown-linux-musl](https://img.shields.io/badge/target-aarch64--musl-brightgreen.svg)]()

Production-grade, sovereign node core for the **Kuroshio Gyre** decentralized ecosystem. Designed for bare-metal execution on Raspberry Pi 4/5, SBCs, and 64-bit ARM edge nodes, Atlantis couples **Token-2022 double-entry accounting**, **OpenRaft distributed consensus**, **Zero-Knowledge HTLC geofencing**, and a live terminal dashboard (**Ratatui TUI**).

---

## ⚡ 1-Line Quickstart (Raspberry Pi / SBCs / Linux ARM64)

Install and start the complete Atlantis systemd daemon and node core in one line:

```bash
curl -sSL https://raw.githubusercontent.com/Neptune537/atlantis-node/master/scripts/install.sh | sudo bash
```

### For Merchant / POS Store Nodes:
```bash
curl -sSL https://raw.githubusercontent.com/Neptune537/atlantis-node/master/scripts/provision_store_node.sh | sudo bash
```

---

## 🏛️ System Architecture

```
                                  [ Kuroshio Mesh / P2P Transport ]
                                                 │
                                                 ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ ATLANTIS NODE (Tier 3 Sovereign Edge Hub)                                              │
│                                                                                        │
│  [ crates/home_node ]                                                                  │
│   • Actix-Web / Axum REST & WebSocket Gateways                                         │
│   • Google OAuth OpenID JWKS DID Binding (Entropy-enforced verification)               │
│   • Ratatui Real-Time TUI Dashboard (Raft peers, ledger telemetry, mesh activity)      │
│                                                                                        │
│  [ crates/tide_ledger ]                                                                │
│   • Zero-Sum Double-Entry Ledger (Invariable balanced journals, non-negative checks)   │
│   • OpenRaft v0.9 Consensus Integration with state-machine snapshotting                │
│                                                                                        │
│  [ crates/kuroshio_mesh ]                                                              │
│   • Zero-Knowledge Geofence Bounties (Groth16 transcript witness checks)               │
│   • Noise-authenticated peer-to-peer transport                                         │
│                                                                                        │
│  [ crates/olympus & atlantis_protocol ]                                                │
│   • High-entropy secure enclave key derivation (PearlEnclaveIdentity)                  │
│   • Cryptographic DID issuance and cross-crate protocol wire types                     │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📦 Workspace Crates

| Crate | Path | Responsibility |
| :--- | :--- | :--- |
| **`home_node`** | [`crates/home_node`](crates/home_node) | Primary binary service, REST endpoints, Ratatui TUI, and hardware watchdog integration. |
| **`tide_ledger`** | [`crates/tide_ledger`](crates/tide_ledger) | Sovereign double-entry bookkeeping engine and OpenRaft consensus cluster state machine. |
| **`kuroshio_mesh`**| [`crates/kuroshio_mesh`](crates/kuroshio_mesh)| P2P mesh transport, HTLC time-locks, and Groth16 zero-knowledge location proofs. |
| **`atlantis_protocol`** | [`crates/atlantis_protocol`](crates/atlantis_protocol) | Canonical protocol schemas, wire envelopes, and cryptographic DID bindings. |
| **`olympus`** | [`crates/olympus`](crates/olympus) | Root authority keystone, Pearl Enclave salt derivation, and anchor verification. |
| **`solana_programs`** | [`crates/solana_programs`](crates/solana_programs) | On-chain Anchor / Token-2022 smart contract programs for Solana settlement. |
| **`ai_gate`** | [`crates/ai_gate`](crates/ai_gate) | Subsurface LLM gateway and inference bridge for pelagic edge workers. |

---

## 🛠️ Building From Source

### Prerequisites
- Rust 1.80+ (`stable`)
- `cross` (if cross-compiling for ARM/aarch64)

```bash
# Clone the repository
git clone https://github.com/Neptune537/atlantis-node.git
cd atlantis-node

# Run full test suite across workspace crates
cargo test --workspace

# Build optimized binary
cargo build --release -p home_node
```

### Cross-Compiling for aarch64 (Raspberry Pi 4 / 5):
```bash
cross build --target aarch64-unknown-linux-musl --release -p home_node
```

---

## 🔐 Security & Hardening

Atlantis enforces strict production-level cryptographic standards:
- **Zero-Sum Ledger Integrity:** All credit/debit transaction pairs must balance to exactly zero within integer micro-units.
- **ZK Geofence Proofs:** Groth16 zero-knowledge proofs enforce cryptographic transcript binding to prevent GPS spoofing and replay attacks.
- **Entropy Enclave Derivation:** Master seeds and authority keys are derived with high-entropy salt and secure environment variables, never hardcoded strings.
- **Strict JWT Verification:** OAuth claims strictly parse 3-part signed tokens with signature entropy guarantees.

---

## 📄 License
MIT © Kuroshio Gyre Ecosystem
