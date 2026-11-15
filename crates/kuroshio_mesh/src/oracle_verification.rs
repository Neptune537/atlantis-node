//! Local policy boundary for a future Reticulum/MCP adapter. No network or ledger writes.
//! Proof-of-History freshness is distinct from Sebastian's Proof-of-Health telemetry.
//! The trusted host must validate signatures, chain continuity, tile binding and replay
//! protection before supplying `Verified`. Guest code cannot establish that trust.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence { Pending, Verified, Invalid }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution { Anchored, VisualAnchored, Degraded, Pending, Rejected }

#[derive(Debug, Clone, Copy)]
pub struct OraclePolicy {
    /// Only non-critical visual assets may use degraded resolution.
    pub non_critical: bool,
    pub budget: Duration,
}

impl OraclePolicy {
    /// Pure, non-blocking evaluation. `anchor_age` includes transport and verification
    /// time, measured by a trusted monotonic host; `uncertainty` bounds clock error.
    /// A caller-supplied timestamp or RTC correction alone is not a trusted anchor.
    pub fn oracle_verification(
        self, evidence: Evidence, anchor_age: Duration, uncertainty: Duration,
    ) -> Resolution {
        if evidence == Evidence::Invalid || self.budget.is_zero() {
            return Resolution::Rejected;
        }
        let budget = if self.non_critical { self.budget }
            else { self.budget.min(Duration::from_millis(250)) };
        let fresh = anchor_age.checked_add(uncertainty)
            .is_some_and(|age| age < budget);
        match (evidence, fresh, self.non_critical) {
            (Evidence::Verified, true, false) => Resolution::Anchored,
            (Evidence::Verified, true, true) => Resolution::VisualAnchored,
            (Evidence::Pending, true, _) => Resolution::Pending,
            (_, false, true) => Resolution::Degraded,
            _ => Resolution::Rejected,
        }
    }
}

impl Resolution {
    /// Visual QoS and degradation never authorize occupancy, ownership or money transitions.
    pub fn permits_authoritative_transition(self) -> bool { self == Self::Anchored }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_uncertainty_and_tier_matrix() {
        for non_critical in [false, true] {
            let policy = OraclePolicy { non_critical, budget: Duration::from_millis(250) };
            for age in [0, 249, 250, 251, u64::MAX] {
                for evidence in [Evidence::Pending, Evidence::Verified, Evidence::Invalid] {
                    let result = policy.oracle_verification(evidence, Duration::from_millis(age), Duration::ZERO);
                    assert_eq!(result.permits_authoritative_transition(), !non_critical && evidence == Evidence::Verified && age < 250);
                    if evidence == Evidence::Invalid { assert_eq!(result, Resolution::Rejected); }
                    if age >= 250 && evidence != Evidence::Invalid {
                        assert_eq!(result, if non_critical { Resolution::Degraded } else { Resolution::Rejected });
                    }
                }
            }
            assert_eq!(policy.oracle_verification(Evidence::Verified, Duration::from_millis(249), Duration::from_millis(1)),
                if non_critical { Resolution::Degraded } else { Resolution::Rejected });
        }
        let strict = OraclePolicy { non_critical: false, budget: Duration::from_secs(10) };
        assert_eq!(strict.oracle_verification(Evidence::Verified, Duration::from_millis(250), Duration::ZERO), Resolution::Rejected);
        assert_eq!(strict.oracle_verification(Evidence::Verified, Duration::MAX, Duration::MAX), Resolution::Rejected);
        let visual = OraclePolicy { non_critical: true, budget: Duration::from_secs(1) };
        let result = visual.oracle_verification(Evidence::Verified, Duration::from_millis(500), Duration::ZERO);
        assert_eq!(result, Resolution::VisualAnchored);
        assert!(!result.permits_authoritative_transition());
    }
}
