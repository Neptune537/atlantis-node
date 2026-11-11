## Atlantis Node v0.2.0 Release

### Highlights
- **Production Security Hardening:**
  - Removed auto-seeding mock balances; payments strictly require real escrow/account balance (HTTP 402 rejection).
  - Enforced authentic Ed25519 signatures across mesh envelope dispatch and settlement verification.
  - Hardened Google JWKS JWT parsing with strict 3-part header/payload/signature validation and high-entropy signature checks.
  - ZK-SNARK Groth16 geofence proof verification: transcript pairing checks to prevent proof forgery and GPS replay attacks.
  - High-entropy secure enclave key derivation (`PearlEnclaveIdentity::from_secure_env`) replacing static string seeds.
- **Cross-Compilation:**
  - Statically linked Musl `aarch64-unknown-linux-musl` release binary (`atlantis-node-aarch64`) for Raspberry Pi 4/5 and 64-bit ARM Linux.
  - Verified SHA256 checksums included.

### Installation
```bash
curl -sSL https://raw.githubusercontent.com/Neptune537/atlantis-node/master/scripts/install.sh | sudo bash
```
