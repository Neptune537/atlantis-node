# AGENTS.md

Local auth source update 2026-11-14: `/api/auth/node-browser/capability` verifies
the caller's bearer session and optional `ATLANTIS_NODE_BROWSER_OWNER_DID` owner
binding, but always reports the remote browser unavailable. It does not issue
browser sessions, start a worker, or stream a desktop. Keep those operations
disabled until owner authorization, worker isolation and private transport are
implemented and tested. The route is not deployed.

## Current integration and funding record

Reviewed 2026-11-11. Read [platform runtime status](../../PLATFORM_RUNTIME_STATUS.md) and the [CYAN economy proposal](CYAN_ECONOMY_PROPOSAL.md) before changing identity, wallet, membership or settlement behavior. The feature catalog below is a platform roadmap.

The agreed membership is $12 per user annually after official launch, including store owners. Stores also contribute 10% of eligible income. Preview activity is free. Keep launch gating, membership dues and store settlement distinct in implementation and reporting.

Treat Google/Apple identity alignment and wallet-link code as requiring security integration review. Google RS256 now requires signatures verified with fresh issuer-scoped keys fetched from the fixed provider HTTPS endpoint. Client-uploaded keys and production EdDSA/EC provider tokens are rejected. Opaque bearer sessions now persist as hashes in SQLite, with expiry and logout revocation. Wallet APIs require a bearer-session principal matching the target and an exact registered one-time challenge. The HTTP wallet/account/challenge paths now persist transactionally in SQLite; older verifier helpers remain memory-backed. Both web shells use a shared bearer-session client. Apple key loading, native identity bridging and real browser/device login acceptance remain unfinished. Account alignment is disabled until durable migration support exists. Require verification against trusted provider keys and authorized, durable account state before exposing financial or private-data operations. Record deployed mint and contract evidence separately from Rust reference models.

Instructions, platform architecture, and roadmap of features for the **Atlantis Platform** (Social Clubs & Entrepreneur Network).

---

## 1. Overview & Vision

Atlantis is a decentralized, high-performance ecosystem designed for two interconnected groups:
1. **Social Clubs & Communities:** Film buffs, book clubs, Comic-Con & cosplay circles, nightlife scenes, and local gatherings.
2. **Entrepreneurs & Creators:** App builders, venue owners, local businesses, and third-party developers extending the Atlantis network.

---

## 2. Platform Feature Catalog

Agents and developers building on Atlantis must implement and reference the following core feature sets:

### 2.1. Social Club Experience Modules

Organizer-activatable feature modules tailored to specific club subcultures:

* **Movie & Screening Clubs:**
  * **Watchlist & Sync:** Integration with watchlists (Letterboxd/Trakt style) and community voting on upcoming screenings.
  * **Ticketing & Venue Pooling:** Group seat reservations and synchronized virtual watch-parties with real-time reactions.
  * **Spoiler Zones:** Post-screening discussion threads hidden behind dynamic "Mark as Watched" verification gates.
* **Book & Reading Clubs:**
  * **Reading Cadence & Pacing Tracker:** Milestone schedules (e.g., Chapters 1–5 by Friday) with community progress meters.
  * **Interactive Quotations & Prompts:** Pin quotes, chapter notes, and debate topics linked to specific pages/chapters.
* **Comic-Con, Cosplay & Fandoms:**
  * **Cosplay & Build Showcase:** Portfolios showing WIP (Works-in-Progress), bill-of-materials, and tagged craftspeople.
  * **Floor Map & Meetup Beacons:** Real-time localized beacons to assemble members on convention floors without group chat spam.
  * **Swag & Prop Exchange:** Peer-to-peer trading boards for pins, badges, artist prints, and collectibles.
* **Nightlife & Party Scenes:**
  * **Digital Door Passes:** Encrypted QR passes compatible with Apple Wallet/Google Wallet with door-scanner verification.
  * **Vibe Check & Crowd Radar:** Live telemetry on queue times, music genres, and active performers/DJs.
  * **Crowd Radar Data Fusion Engine:** Multi-source sensor & telemetry fusion combining door pass check-ins, beacon taps, ambient venue sensors, and member observations weighted by Proof-of-Health (PoH) scores and half-life decay.
  * **Trust & Safety Vouching:** Community approval queues and peer vouches for high-trust, curated gatherings.

---

### 2.2. Universal Club Operations & Engagement

Core utilities available across all communities:

* **Event Orchestration & Ticketing:**
  * Capacity caps, waitlists, plus-one limits, and commitment deposits to prevent no-shows.
  * Native payment gateway integration (fiat/crypto/Atlantis ledger credits) with automated refunds.
* **Tiered Memberships & Subscriptions:**
  * Configurable tiers (e.g., Guest, Active Member, VIP/Patron) granting early ticket access, private channels, and perks.
* **Smart Roles & Governance:**
  * Role hierarchies: Host, Curator, Moderator, Door Staff, Member, Guest.
  * On-chain/in-app polling for community decisions (event venues, budgets, schedule choices).
* **Interactive Club Communication:**
  * Ephemeral event-specific chat rooms (active 24 hours prior to 48 hours post-event).
  * Structured, topic-based discussion boards replacing noisy single-stream messaging.

---

### 2.3. Entrepreneur & Developer Network ("Atlantis Network")

Capabilities enabling entrepreneurs to plug their applications, services, and businesses into Atlantis:

* **Mini-App / Extension Framework:**
  * Embedded webview & native extension support (trivia bots, drink ordering, ride/carpool coordination).
  * App directory where clubs can install extensions with 1-click permissioning.
* **Atlantis Developer SDK & Open APIs:**
  * **Atlantis ID (OAuth / Decoupled Identity):** "Sign in with Atlantis" carrying reputation, verified memberships, and ticket history.
  * **Event & Ticket Webhooks:** Real-time event notifications enabling external vendors (caterers, DJs, photographers, venues) to deliver integrated services.
  * **Social Graph API:** Query member interests, mutual affiliations, and attendance patterns while respecting member privacy.
* **Local Business & Venue Directory:**
  * Business profiles allowing local bars, indie cinemas, and event spaces to offer club-exclusive discounts and packages.
  * Automated sponsorship matching between brands and relevant subculture events.
* **Monetization & Split-Payout Engine:**
  * Automated payment splitting among co-hosts, venues, performers, and developers.
  * Revenue-share mechanics for paid mini-apps and club subscription fees.

---

### 2.4. Cross-Club Discovery & Social Passport

* **Federated City Discovery Map:**
  * Real-time map displaying public subculture gatherings, open club events, and hot spots.
  * "Club Crossovers": Native co-hosting tooling for collaborative events between two or more clubs.
* **Decentralized Social Passport:**
  * Portable badges, Proof-of-Attendance (POAP), and reputational credentials across clubs.
  * Instant, contact-safe exchange via QR code/NFC for real-world interactions.

---

## 3. Developer & Agent Implementation Instructions

When building, generating code, or scaffolding features for Atlantis, follow these guidelines:

1. **Architecture & Crates:**
   - **`crates/atlantis_protocol`**: Define shared data structures, events, DTOs, and RPC interfaces (e.g., club definitions, event schemas, ticket passes).
   - **`crates/tide_ledger`**: Handle transactions, balance transfers, split payments, and membership records.
   - **`crates/home_node`**: Run peer services, local storage, club indexing, event synchronization, the Nightlife Crowd Radar Data Fusion engine (`fusion.rs`), and the **24/7 Seven Daughters Siren Radio daemon** (`fortress_theater_service.rs`, `SevenDaughtersPiSchedule`) auto-started whenever `ClubHomeNode` runs on the Club's Raspberry Pi LAN node (`10.0.0.237:8080`).
   - **`crates/ai_gate`**: Interface for LLM/agentic workflows (e.g., automated event curation, vibe summaries, recommendation algorithms).
   - **`crates/solana_programs`**: Manage on-chain smart contracts, badges/credentials, and ticket settlement where decentralized verification is required.

2. **Security & Data Privacy:**
   - Always prioritize privacy for member locations. Obfuscate exact coordinates for crowd radars and beacons until opt-in conditions are met.
   - Ensure ticket QR codes use cryptographic signing (Ed25519) to prevent ticket duplication or forgery.
   - Implement strict permission checks on all Mini-App API access points using OAuth scoped tokens.

3. **Modularity First:**
   - Design club features as pluggable components. A book club should only load reading tracker modules without loading nightlife crowd radar modules unless configured by the community admin.

Auth deployment reviewed 2026-11-11 Chicago: seed Pi zero@10.0.0.60 runs a separate auth-only container on loopback 8087, routed through the apex /api/auth paths. Google client ID and API origin are configured in the shared web clients. The portal Firebase popup was replaced with the Google FedCM button; preserve the game isolation headers. Public negative-auth/CORS checks passed; real Google account/device acceptance remains open, and Sonar still returned 403. See the platform runtime status and Atlantis/deploy/auth_seed_release_20261111.json for the dated record.
