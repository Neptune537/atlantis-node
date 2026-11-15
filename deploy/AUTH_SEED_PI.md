# Seed Pi authentication service

The public origin is https://tr7n7t7.com. The authorized Google web client ID is 615673989451-f294t6230ml283pmcrd4uqcgmbd8m9jp.apps.googleusercontent.com. These are public deployment values, not OAuth client secrets. Do not put client secrets in either web app.

Initial inspection on 2026-11-11 found the seed Pi at zero@10.0.0.60. Its apex tunnel routed every path to Python static hosting on 127.0.0.1:8085. Browser-style GET /api/auth/me returned HTML 404 and POST /api/auth/oidc returned 501. The legacy Home Node container on 8081 also lacked these auth routes.

The separate auth_api binary provides only the /api/auth endpoints. It shares tested identity and SQLite code with home_node, and adds an explicit browser-origin allowlist. Generic ledger, marketplace and daemon-control APIs are not part of this router. Configure GOOGLE_OAUTH_CLIENT_ID, ATLANTIS_SESSION_DB, ATLANTIS_AUTH_ORIGINS and ATLANTIS_AUTH_BIND. The ingress snippet routes only the listed apex auth paths to loopback port 8087 before the existing static-site rule.

Stage source under /home/zero/atlantis-auth/releases/20261111. Keep the auth database in /home/zero/atlantis-auth/data with service-user-only permissions and a persistent mount. The independent container should publish only 127.0.0.1:8087, run as uid 1000, drop capabilities, use a read-only root filesystem and restart unless stopped. Its source/build must not replace the legacy container or its data volume.

Verify local unauthenticated /api/auth/me and invalid-token login return 401, allowed-origin preflight succeeds, untrusted origins fail, and non-auth daemon routes return 404 before adding ingress. Back up the active tunnel config and validate cloudflared ingress rules before restarting the user tunnel service. Recheck public CORS and auth rejection, radio and Sonar after cutover. Restore the backed-up config and restart the tunnel if the cutover fails.

Wallet linking stores a verified proof-of-control association in SQLite. It does not mint a token or send an on-chain transaction. Real Google sign-in requires the authorized browser origins registered in Google Cloud. Real account, wallet-provider, phone and Quest acceptance remain separate from invalid-token checks.

Deployment completed 2026-11-11 Chicago: atlantis-auth-api is running with restart policy unless-stopped. The auth rule is installed and public negative-auth/CORS checks passed. Both web clients and the apex portal serve their tested account scripts. Radio remains reachable; Sonar upgrade still returns 403. See auth_seed_release_20261111.json for provenance. The current binary is a debug build; release-image optimization and load tests remain required.

Club room rollout 2026-11-14: a new release at /home/zero/atlantis-auth/releases/20261113-club runs as atlantis-auth-api with the existing SQLite mount. The old container is stopped as atlantis-auth-api-before-club. A narrow separate tunnel rule routes /api/auth/club/(presence|leave|messages) to loopback 8087; existing auth and other ingress rules are preserved. The previous SQLite snapshot and tunnel config are retained at the paths recorded in auth_club_release_20261114.json. Public negative-auth, CORS, radio and signed Sonar checks passed. A two-session flow was tested only against an isolated canary database, then its test rows and container were removed. Real Google two-account and device acceptance remain open.

