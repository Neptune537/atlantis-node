# Home Node authentication integration

Reviewed 2026-11-11. Google RS256 verification uses ring and Google's fixed HTTPS JWKS endpoint. Set GOOGLE_OAUTH_CLIENT_ID to the registered client ID. Keys load on login and after cache expiry. Unknown keys fail closed until refresh. Apple key loading is not implemented.

## Login and sessions

POST /api/auth/oidc accepts {"id_token":"..."}. Its session_token is now a random 256-bit opaque bearer token. SQLite stores its SHA-256 hash, principal, absolute expiry and last activity. The lifetime is at most one hour and never exceeds the verified identity token expiry; inactivity expires a session after 15 minutes. Login responses include expires_at_s. API responses carry Cache-Control: no-store.

Use Authorization: Bearer <session_token> for wallet operations. Tokens in bodies, query strings and cookies do not authorize these routes. Keep the token in client memory and send it only over HTTPS outside loopback development. Do not log tokens or put them in URLs/localStorage.

Wallet challenge JSON contains principal_did, wallet_pubkey_hex and optional ttl_s. Wallet link JSON contains challenge and signature_hex. The authenticated session principal must own the challenge; the wallet signature and exact registered one-time challenge remain required. Body id_token is no longer an authorization mechanism for these routes.

POST /api/auth/logout accepts {"all_sessions":false} to revoke the caller's token, or {"all_sessions":true} to revoke every session belonging to the caller. Sessions and revocation survive process restart.

ATLANTIS_SESSION_DB selects the SQLite path; default: data/auth-sessions.sqlite relative to the daemon working directory. Startup fails if the database cannot be opened. Use a persistent service-owned directory with restrictive OS permissions; exclude it from public web roots and source control. This is a single-host session store, not multi-node failover replication. Backups contain account identifiers and session hashes and require private storage.

## Remaining integration work

POST /api/auth/jwks-sync returns 403. Account alignment also returns 403 pending durable account migration support. Provider key trust is never restored from serialized helper data.

The active HTTP routes now persist account principals, wallet links and challenges in the same SQLite database. Wallet ownership is unique; challenge consumption and linking commit in one transaction. One active challenge per account is allowed with a maximum five-minute lifetime. Existing wallet replacement is rejected pending an authorized recovery flow. The older verifier-only helpers remain memory-backed and are not the active wallet API store. Both web shells now share platform-auth.js, using in-memory bearer sessions, real Google Identity Services callbacks and signed injected-wallet challenges. Configure apiOrigin and googleClientId in each auth-config.json; blank configuration explicitly disables account sign-in. No session or identity is restored from client localStorage. The native egui account screen starts as Guest; authenticated native identity bridging remains unfinished. Browser CSRF/nonce handling, real Google two-account login, device handoff, rate limits and deployment acceptance remain release work. Other daemon routes still need their own authorization audit. No financial or whole-platform production readiness is implied.

## Validation

```powershell
cargo test -p home_node --offline --manifest-path "Kuroshio Gyre/Atlantis/Cargo.toml"
```

On 2026-11-11, 26 library tests and one API-handler test passed, and four shared web-client tests passed. Session tests cover restart, revocation, idle/absolute expiry, hash storage and distinct account tokens. The handler test rejects unauthenticated and cross-account wallet operations, accepts the owner with a valid wallet signature, rejects replay and verifies logout isolation. Durable wallet tests cover restart, expired/consumed challenges and cross-account wallet uniqueness. These accounts are test fixtures, not real Google sign-ins. The opt-in google_jwks_live_refresh test passed against Google HTTPS in the preceding verification pass; it remains ignored in offline runs.

References: [Google token verification](https://developers.google.com/identity/gsi/web/guides/verify-google-id-token), [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

GET /api/auth/me returns the authenticated principal_did and persisted linked_solana_wallet (hex public key). Client UI converts the key to Base58 for Solana RPC display. Website config requires an origin only; HTTPS is required except loopback development. Registered Google origins and reachable HTTPS ingress must match the deployment.

## Deployed auth-only service

The standalone src/bin/auth_api.rs binary is now deployed to the seed Pi on loopback port 8087. Unlike the broader development daemon, its router contains only /api/auth endpoints and uses an explicit browser-origin allowlist. Public checks pass for missing/invalid credentials, allowed CORS preflight and blocked foreign origins. See ../../deploy/AUTH_SEED_PI.md and ../../deploy/auth_seed_release_20261111.json. The production values in both auth-config.json files are configured and published.

The apex portal's legacy Firebase signInWithPopup integration has been removed. All web entry points now use Google's official sign-in button with use_fedcm_for_button enabled; no custom window.closed polling is used. The game keeps same-origin opener isolation. Real account sign-in and browsers without FedCM support still need acceptance testing. A successful negative-auth check does not verify an account login.

Portal routing update: the public portal login button opens https://heavenlybound-lite.pages.dev/?signin=google. Sign-in completes inside Lite, keeping its bearer session in that app memory; no bearer token is placed in the URL or copied into browser storage. The query carries only an allowlisted provider intent. Cross-app persistent sign-on/handoff is still separate work. The legacy /game.html entry forwards to the single current HeavenlyBound release.
