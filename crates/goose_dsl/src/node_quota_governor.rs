//! # Kuroshio Relative Resource & Node Compute Quota Governor
//!
//! Enforces: "Each user will have compute power relative to the resources tied to their nodes."
//! - Computes node compute weight based on CPU cores, GPU render nodes, and VRAM/RAM.
//! - Allocates fractional execution slices for 3D physics, audio DSP, and cloud jobs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResourceShare {
    pub user_did: String,
    pub node_id: String,
    pub cpu_cores: usize,
    pub gpu_vram_mb: u64,
    pub total_ram_mb: u64,
    pub compute_weight_score: f64,
    pub allocated_mesh_bandwidth_pct: f64,
}

impl NodeResourceShare {
    pub fn calculate(user_did: &str, node_id: &str, cores: usize, gpu_vram_mb: u64, ram_mb: u64) -> Self {
        // Compute relative weight formula:
        // Score = (cores * 10) + (gpu_vram_mb / 128) + (ram_mb / 256)
        let score = (cores as f64 * 10.0) + (gpu_vram_mb as f64 / 128.0) + (ram_mb as f64 / 256.0);
        
        Self {
            user_did: user_did.to_string(),
            node_id: node_id.to_string(),
            cpu_cores: cores,
            gpu_vram_mb,
            total_ram_mb: ram_mb,
            compute_weight_score: score,
            allocated_mesh_bandwidth_pct: 0.0, // normalized across cluster
        }
    }
}

pub fn rebalance_cluster_shares(nodes: &mut [NodeResourceShare]) {
    let total_score: f64 = nodes.iter().map(|n| n.compute_weight_score).sum();
    if total_score > 0.0 {
        for node in nodes.iter_mut() {
            node.allocated_mesh_bandwidth_pct = (node.compute_weight_score / total_score) * 100.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_resource_allocation() {
        let mut cluster = vec![
            NodeResourceShare::calculate("did:atlantis:seed", "pi-node-01", 4, 0, 4096),
            NodeResourceShare::calculate("did:atlantis:screen", "pi-node-02", 4, 0, 8192),
            NodeResourceShare::calculate("did:atlantis:owner", "workstation-host", 8, 4096, 14336),
        ];

        rebalance_cluster_shares(&mut cluster);
        
        // Workstation has discrete Iris Xe MAX GPU + 8 threads, should command majority share
        assert!(cluster[2].allocated_mesh_bandwidth_pct > cluster[0].allocated_mesh_bandwidth_pct);
    }
}
