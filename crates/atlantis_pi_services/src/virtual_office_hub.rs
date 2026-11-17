//! # Virtual Office 3D Spatial Totem & Agent Dispatcher
//!
//! Maps each executive agent to a 3D coordinate desk inside the Virtual Studio/Office
//! and binds their autonomous tasks to node hardware.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutiveOfficer {
    pub name: String,
    pub title: String,
    pub room_coordinate_xyz: [f32; 3],
    pub primary_focus: String,
    pub authority_level: String,
    pub hardware_binding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualOfficeState {
    pub office_name: String,
    pub officers: Vec<ExecutiveOfficer>,
    pub active_directive: String,
    pub revenue_target_monthly_usd: f64,
}

impl VirtualOfficeState {
    pub fn new_founder_suite() -> Self {
        let triton = ExecutiveOfficer {
            name: "Triton".to_string(),
            title: "Chief of Staff & Cluster Governor".to_string(),
            room_coordinate_xyz: [0.0, 1.2, -3.0], // Center executive desk facing entrance
            primary_focus: "Node uptime, OpenRaft consensus, FIPS air-gap readiness, agent task routing".to_string(),
            authority_level: "High - Fleet Commander".to_string(),
            hardware_binding: "Pi Seed Node (10.0.0.58) & Host Workstation".to_string(),
        };

        let sebastian = ExecutiveOfficer {
            name: "Sebastian".to_string(),
            title: "Director of Marketing & Sales".to_string(),
            room_coordinate_xyz: [-2.5, 1.2, -1.5], // Left media lounge / cinema projection desk
            primary_focus: "TR7N7T7 audience growth, $12/yr Coral VIP conversions, Siren Radio syndication".to_string(),
            authority_level: "Operational - Commercial Outreach".to_string(),
            hardware_binding: "Pi Screen Node (10.0.0.60) / Public Gate".to_string(),
        };

        let ursula = ExecutiveOfficer {
            name: "Ursula".to_string(),
            title: "Chief Growth Strategist & Capital Allocator".to_string(),
            room_coordinate_xyz: [2.5, 1.2, -1.5], // Right trading / analytics terminal
            primary_focus: "Charter Cyan yield farming, Coral Escrow velocity, SBIR/Federal grant positioning".to_string(),
            authority_level: "High - Treasury & Market Strategy".to_string(),
            hardware_binding: "Host Dual-GPU Heavy Compute Node".to_string(),
        };

        Self {
            office_name: "Kuroshio Sanctum Virtual Office".to_string(),
            officers: vec![triton, sebastian, ursula],
            active_directive: "Autonomous Node Monetization & Zero-Cost Federal Scaling".to_string(),
            revenue_target_monthly_usd: 12000.0,
        }
    }
}
