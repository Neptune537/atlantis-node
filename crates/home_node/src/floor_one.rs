//! Staging-only authoritative floor-one rules. No HTTP route, currency or client save import.
//! The host supplies authenticated sessions, a registered Nautilus overlay's ECEF
//! geometry, accepted physics poses and monotonically advancing 60 Hz simulation ticks.
use crate::sessions::SessionStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

const MAX_PLAYERS: usize = 2;
const MAX_STAMINA: u16 = 100;
const POSE_TTL: u64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    WaterSeal,
    FireSeal,
    StrikeGuardian,
}

/// Deliberately has no principal, position, damage, reward or server-time field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub match_id: Uuid,
    pub sequence: u64,
    pub client_tick: u64,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub match_id: Uuid,
    pub revision: u64,
    pub server_tick: u64,
    pub next_sequence: u64,
    pub stamina: u16,
    pub active_in_scene: bool,
    pub water_seal: bool,
    pub fire_seal: bool,
    pub guardian_health: u16,
    /// Nontransferable, nonfinancial shared match completion only.
    pub completed: bool,
}

/// Server-only scene registration referencing an existing AnchoredSpatialOverlay.
/// This module does not create a floating scene or publish its geometry.
pub struct Scene {
    pub overlay_id: String,
    pub anchor_ecef: [f64; 3],
    pub water_ecef: [f64; 3],
    pub fire_ecef: [f64; 3],
    pub guardian_ecef: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("authentication required")]
    Authentication,
    #[error("not a match member")]
    Membership,
    #[error("match is full")]
    Capacity,
    #[error("invalid registered scene or accepted pose")]
    Geometry,
    #[error("wrong match")]
    WrongMatch,
    #[error("input replay, conflict or sequence gap")]
    Sequence,
    #[error("invalid simulation clock or stale input")]
    Clock,
    #[error("fresh accepted pose required")]
    Pose,
    #[error("target outside interaction range or obstructed")]
    Range,
    #[error("technique is recovering")]
    Cooldown,
    #[error("insufficient stamina")]
    Stamina,
    #[error("objective prerequisites not met or already complete")]
    Objective,
    #[error("counter exhausted")]
    Exhausted,
}

struct Player {
    active: bool,
    pose: [f64; 3],
    pose_tick: Option<u64>,
    // Target visibility is calculated by host physics, never taken from an intent.
    visible: [bool; 3],
    stamina: u16,
    ready_tick: u64,
    next_sequence: u64,
    last_receipt: Option<(Intent, Snapshot)>,
}

pub struct FloorOne {
    id: Uuid,
    scene: Scene,
    tick: u64,
    revision: u64,
    water: bool,
    fire: bool,
    health: u16,
    players: BTreeMap<String, Player>,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn finite(p: [f64; 3]) -> bool {
    p.into_iter().all(f64::is_finite)
}

impl FloorOne {
    pub fn new(id: Uuid, scene: Scene) -> Result<Self, Error> {
        let radius = distance(scene.anchor_ecef, [0.; 3]);
        if id.is_nil()
            || scene.overlay_id.is_empty()
            || scene.overlay_id.len() > 128
            || !finite(scene.anchor_ecef)
            || !(6_000_000. ..=7_000_000.).contains(&radius)
            || [scene.water_ecef, scene.fire_ecef, scene.guardian_ecef]
                .into_iter()
                .any(|p| !finite(p) || distance(p, scene.anchor_ecef) > 100.)
        {
            return Err(Error::Geometry);
        }
        Ok(Self {
            id,
            scene,
            tick: 0,
            revision: 0,
            water: false,
            fire: false,
            health: 100,
            players: BTreeMap::new(),
        })
    }

    /// Trusted simulation scheduler only. No client route should expose this API.
    pub fn advance(&mut self, tick: u64) -> Result<(), Error> {
        if tick < self.tick {
            return Err(Error::Clock);
        }
        // Regenerate one stamina per complete six-tick interval, without losing
        // fractional intervals when the host advances one tick at a time.
        let gain = ((tick / 6) - (self.tick / 6)).min(MAX_STAMINA as u64) as u16;
        for player in self.players.values_mut() {
            player.stamina = player.stamina.saturating_add(gain).min(MAX_STAMINA);
        }
        self.tick = tick;
        Ok(())
    }

    /// Host admission after authenticating the bearer. A reconnect retains state.
    pub fn join(
        &mut self,
        sessions: &mut SessionStore,
        bearer: &str,
        now_s: u64,
    ) -> Result<Snapshot, Error> {
        let principal = sessions
            .authenticate(bearer, now_s)
            .map_err(|_| Error::Authentication)?;
        if !self.players.contains_key(&principal) {
            if self.players.len() == MAX_PLAYERS {
                return Err(Error::Capacity);
            }
            self.players.insert(
                principal.clone(),
                Player {
                    active: false,
                    pose: self.scene.anchor_ecef,
                    pose_tick: None,
                    visible: [false; 3],
                    stamina: MAX_STAMINA,
                    ready_tick: 0,
                    next_sequence: 1,
                    last_receipt: None,
                },
            );
        }
        self.view(&principal)
    }

    /// Trusted host admission/departure only. Joining authenticates a public-match
    /// member but cannot activate their scene membership. Both transitions discard
    /// the prior pose; re-admission requires a new accepted physics sample.
    pub fn set_scene_presence(&mut self, principal: &str, active: bool) -> Result<(), Error> {
        let player = self.players.get_mut(principal).ok_or(Error::Membership)?;
        player.active = active;
        player.pose_tick = None;
        player.visible = [false; 3];
        Ok(())
    }

    /// Trusted physics adapter only: accepted virtual pose and three target raycasts.
    /// The adapter must enforce movement/collision and scene membership before calling.
    pub fn set_server_pose(
        &mut self,
        principal: &str,
        pose: [f64; 3],
        visible: [bool; 3],
    ) -> Result<(), Error> {
        if !finite(pose) || distance(pose, self.scene.anchor_ecef) > 100. {
            return Err(Error::Geometry);
        }
        let player = self.players.get_mut(principal).ok_or(Error::Membership)?;
        if !player.active {
            return Err(Error::Membership);
        }
        player.pose = pose;
        player.pose_tick = Some(self.tick);
        player.visible = visible;
        Ok(())
    }

    pub fn snapshot(
        &self,
        sessions: &mut SessionStore,
        bearer: &str,
        now_s: u64,
    ) -> Result<Snapshot, Error> {
        let principal = sessions
            .authenticate(bearer, now_s)
            .map_err(|_| Error::Authentication)?;
        self.view(&principal)
    }

    fn view(&self, principal: &str) -> Result<Snapshot, Error> {
        let player = self.players.get(principal).ok_or(Error::Membership)?;
        Ok(Snapshot {
            match_id: self.id,
            revision: self.revision,
            server_tick: self.tick,
            next_sequence: player.next_sequence,
            stamina: player.stamina,
            active_in_scene: player.active,
            water_seal: self.water,
            fire_seal: self.fire,
            guardian_health: self.health,
            completed: self.health == 0,
        })
    }

    /// Authentication is repeated for every operation, including exact retries.
    pub fn submit(
        &mut self,
        sessions: &mut SessionStore,
        bearer: &str,
        now_s: u64,
        intent: Intent,
    ) -> Result<Snapshot, Error> {
        let principal = sessions
            .authenticate(bearer, now_s)
            .map_err(|_| Error::Authentication)?;
        self.apply(&principal, intent)
    }

    fn apply(&mut self, principal: &str, intent: Intent) -> Result<Snapshot, Error> {
        if intent.match_id != self.id {
            return Err(Error::WrongMatch);
        }
        let player = self.players.get(principal).ok_or(Error::Membership)?;
        if !player.active {
            return Err(Error::Membership);
        }
        if let Some((last, receipt)) = &player.last_receipt {
            if *last == intent {
                return Ok(receipt.clone());
            }
        }
        if intent.sequence != player.next_sequence {
            return Err(Error::Sequence);
        }
        if intent.client_tick > self.tick || self.tick - intent.client_tick > 120 {
            return Err(Error::Clock);
        }
        if player
            .pose_tick
            .is_none_or(|tick| self.tick - tick > POSE_TTL)
        {
            return Err(Error::Pose);
        }
        if self.tick < player.ready_tick {
            return Err(Error::Cooldown);
        }
        let (index, target, cost, recovery) = match intent.action {
            Action::WaterSeal if !self.water => (0, self.scene.water_ecef, 0, 12),
            Action::FireSeal if !self.fire => (1, self.scene.fire_ecef, 0, 12),
            Action::StrikeGuardian if self.water && self.fire && self.health > 0 => {
                (2, self.scene.guardian_ecef, 30, 30)
            }
            _ => return Err(Error::Objective),
        };
        if !player.visible[index] || distance(player.pose, target) > 2.5 {
            return Err(Error::Range);
        }
        if player.stamina < cost {
            return Err(Error::Stamina);
        }
        let next = player
            .next_sequence
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        let revision = self.revision.checked_add(1).ok_or(Error::Exhausted)?;
        let ready = self.tick.checked_add(recovery).ok_or(Error::Exhausted)?;
        // No mutation until all checks and arithmetic succeed.
        let player = self.players.get_mut(principal).unwrap();
        player.next_sequence = next;
        player.stamina -= cost;
        player.ready_tick = ready;
        match intent.action {
            Action::WaterSeal => self.water = true,
            Action::FireSeal => self.fire = true,
            Action::StrikeGuardian => self.health = self.health.saturating_sub(25),
        }
        self.revision = revision;
        let receipt = self.view(principal)?;
        self.players.get_mut(principal).unwrap().last_receipt = Some((intent, receipt.clone()));
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ALICE: &str = "did:atlantis:alice";
    const BOB: &str = "did:atlantis:bob";
    fn setup() -> (FloorOne, SessionStore, String, String) {
        let anchor = [6_378_137., 0., 0.];
        let game = FloorOne::new(
            Uuid::new_v4(),
            Scene {
                overlay_id: "test-registered-overlay".into(),
                anchor_ecef: anchor,
                water_ecef: anchor,
                fire_ecef: anchor,
                guardian_ecef: anchor,
            },
        )
        .unwrap();
        let mut sessions = SessionStore::open(":memory:").unwrap();
        let a = sessions.issue(ALICE, 100, 1000).unwrap().0;
        let b = sessions.issue(BOB, 100, 1000).unwrap().0;
        (game, sessions, a, b)
    }
    fn intent(game: &FloorOne, sequence: u64, action: Action) -> Intent {
        Intent {
            match_id: game.id,
            sequence,
            client_tick: game.tick,
            action,
        }
    }
    fn pose(game: &mut FloorOne, principal: &str) {
        game.set_scene_presence(principal, true).unwrap();
        game.set_server_pose(principal, game.scene.anchor_ecef, [true; 3])
            .unwrap();
    }
    #[test]
    fn two_players_complete_once_and_reconnect_with_independent_sequences() {
        let (mut g, mut s, a, b) = setup();
        g.join(&mut s, &a, 100).unwrap();
        g.join(&mut s, &b, 100).unwrap();
        pose(&mut g, ALICE);
        pose(&mut g, BOB);
        let water = intent(&g, 1, Action::WaterSeal);
        let original = g.submit(&mut s, &a, 100, water.clone()).unwrap();
        assert_eq!(g.submit(&mut s, &a, 100, water).unwrap(), original);
        let fire = intent(&g, 1, Action::FireSeal);
        g.submit(&mut s, &b, 100, fire).unwrap();
        for (tick, who, token, sequence) in [
            (30, ALICE, &a, 2),
            (60, BOB, &b, 2),
            (90, ALICE, &a, 3),
            (120, BOB, &b, 3),
        ] {
            g.advance(tick).unwrap();
            pose(&mut g, who);
            let hit = intent(&g, sequence, Action::StrikeGuardian);
            g.submit(&mut s, token, 100, hit).unwrap();
        }
        let fresh_session = s.issue(ALICE, 101, 1000).unwrap().0;
        let snapshot = g.join(&mut s, &fresh_session, 101).unwrap();
        assert!(snapshot.completed);
        assert_eq!(snapshot.revision, 6);
        assert_eq!(snapshot.next_sequence, 4);
        g.advance(150).unwrap();
        pose(&mut g, ALICE);
        let duplicate = intent(&g, 4, Action::StrikeGuardian);
        assert_eq!(
            g.submit(&mut s, &fresh_session, 101, duplicate),
            Err(Error::Objective)
        );
        assert_eq!(g.revision, 6);
    }
    #[test]
    fn forged_fields_wrong_match_and_session_expiry_fail_closed() {
        let (mut g, mut s, a, b) = setup();
        g.join(&mut s, &a, 100).unwrap();
        pose(&mut g, ALICE);
        let valid = intent(&g, 1, Action::WaterSeal);
        assert_eq!(
            g.submit(&mut s, &b, 100, valid.clone()),
            Err(Error::Membership)
        );
        let mut wrong = valid.clone();
        wrong.match_id = Uuid::new_v4();
        assert_eq!(g.submit(&mut s, &a, 100, wrong), Err(Error::WrongMatch));
        let mut json = serde_json::to_value(&valid).unwrap();
        json["damage"] = 100.into();
        assert!(serde_json::from_value::<Intent>(json).is_err());
        g.submit(&mut s, &a, 100, valid.clone()).unwrap();
        assert_eq!(
            g.submit(&mut s, &a, 1000, valid),
            Err(Error::Authentication)
        );
        assert_eq!(g.revision, 1);
    }
    #[test]
    fn replay_gap_stale_pose_clock_and_obstruction_do_not_mutate() {
        let (mut g, mut s, a, _) = setup();
        g.join(&mut s, &a, 100).unwrap();
        g.set_scene_presence(ALICE, true).unwrap();
        let valid = intent(&g, 1, Action::WaterSeal);
        assert_eq!(g.submit(&mut s, &a, 100, valid.clone()), Err(Error::Pose));
        pose(&mut g, ALICE);
        let mut gap = valid.clone();
        gap.sequence = 2;
        assert_eq!(g.submit(&mut s, &a, 100, gap), Err(Error::Sequence));
        let mut future = valid.clone();
        future.client_tick = 1;
        assert_eq!(g.submit(&mut s, &a, 100, future), Err(Error::Clock));
        g.set_server_pose(ALICE, g.scene.anchor_ecef, [false; 3])
            .unwrap();
        assert_eq!(g.submit(&mut s, &a, 100, valid.clone()), Err(Error::Range));
        g.advance(7).unwrap();
        assert_eq!(g.submit(&mut s, &a, 100, valid.clone()), Err(Error::Pose));
        pose(&mut g, ALICE);
        g.submit(&mut s, &a, 100, valid).unwrap();
        let conflict = intent(&g, 1, Action::FireSeal);
        assert_eq!(g.submit(&mut s, &a, 100, conflict), Err(Error::Sequence));
        assert_eq!(g.revision, 1);
    }
    #[test]
    fn departure_revokes_actions_and_retries_until_host_readmission() {
        let (mut g, mut s, a, _) = setup();
        g.join(&mut s, &a, 100).unwrap();
        let water = intent(&g, 1, Action::WaterSeal);
        assert_eq!(
            g.submit(&mut s, &a, 100, water.clone()),
            Err(Error::Membership)
        );
        pose(&mut g, ALICE);
        g.submit(&mut s, &a, 100, water.clone()).unwrap();
        g.set_scene_presence(ALICE, false).unwrap();
        assert_eq!(g.submit(&mut s, &a, 100, water), Err(Error::Membership));
        assert!(!g.join(&mut s, &a, 100).unwrap().active_in_scene);
        g.advance(12).unwrap();
        g.set_scene_presence(ALICE, true).unwrap();
        let fire = intent(&g, 2, Action::FireSeal);
        assert_eq!(g.submit(&mut s, &a, 100, fire.clone()), Err(Error::Pose));
        pose(&mut g, ALICE);
        g.submit(&mut s, &a, 100, fire).unwrap();
        assert_eq!(g.revision, 2);
    }
    #[test]
    fn bounds_nan_capacity_cooldown_and_stamina_are_enforced() {
        let (mut g, mut s, a, b) = setup();
        g.join(&mut s, &a, 100).unwrap();
        g.join(&mut s, &b, 100).unwrap();
        let c = s.issue("did:atlantis:charlie", 100, 1000).unwrap().0;
        assert_eq!(g.join(&mut s, &c, 100), Err(Error::Capacity));
        assert_eq!(
            g.set_server_pose(ALICE, [f64::NAN; 3], [true; 3]),
            Err(Error::Geometry)
        );
        pose(&mut g, ALICE);
        let hit = intent(&g, 1, Action::StrikeGuardian);
        assert_eq!(g.submit(&mut s, &a, 100, hit), Err(Error::Objective));
        let water = intent(&g, 1, Action::WaterSeal);
        g.submit(&mut s, &a, 100, water).unwrap();
        let fire = intent(&g, 2, Action::FireSeal);
        assert_eq!(g.submit(&mut s, &a, 100, fire), Err(Error::Cooldown));
        g.advance(12).unwrap();
        pose(&mut g, ALICE);
        let fire = intent(&g, 2, Action::FireSeal);
        g.submit(&mut s, &a, 100, fire).unwrap();
        for tick in [42, 72, 102] {
            g.advance(tick).unwrap();
            pose(&mut g, ALICE);
            let seq = g.players[ALICE].next_sequence;
            let hit = intent(&g, seq, Action::StrikeGuardian);
            g.submit(&mut s, &a, 100, hit).unwrap();
        }
        g.advance(132).unwrap();
        pose(&mut g, ALICE);
        let hit = intent(&g, 6, Action::StrikeGuardian);
        assert_eq!(g.submit(&mut s, &a, 100, hit), Err(Error::Stamina));
        assert_eq!(g.health, 25);
        let old = g.tick;
        assert_eq!(g.advance(old - 1), Err(Error::Clock));
        assert_eq!(g.tick, old);
    }
}
