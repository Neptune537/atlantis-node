// Relative Compute Quota Governor in Goose Systems Language
// Allocates processing power relative to physical hardware contributed by each user node

struct UserNode {
    owner_did: u8[..32],
    node_name: u8[..24],
    cpu_cores: i32,
    gpu_vram_mb: i32,
    ram_mb: i32,
    weight_score: f64,
    power_share_pct: f64
}

fn compute_weight(cores: i32, vram: i32, ram: i32) -> f64 {
    // Formula: cores*10 + vram/128 + ram/256
    let score = (cores as f64 * 10.0) + (vram as f64 / 128.0) + (ram as f64 / 256.0);
    return score;
}

fn main() {
    print("================================================================================");
    print("  KUROSHIO GYRE :: RELATIVE COMPUTE POWER ALLOCATION [GOOSE RUNTIME]");
    print("================================================================================");

    var w_pi1 = compute_weight(4, 0, 4096);
    var w_pi2 = compute_weight(4, 0, 8192);
    var w_host = compute_weight(8, 4096, 14336);

    var total_weight = w_pi1 + w_pi2 + w_host;

    var share_pi1 = (w_pi1 / total_weight) * 100.0;
    var share_pi2 = (w_pi2 / total_weight) * 100.0;
    var share_host = (w_host / total_weight) * 100.0;

    print("Node 1 (Seed Pi - 4 Core, 4GB RAM)         : Score = ", w_pi1, " | Power Share = ", share_pi1, "%");
    print("Node 2 (Screen Pi - 4 Core, 8GB RAM)       : Score = ", w_pi2, " | Power Share = ", share_pi2, "%");
    print("Node 3 (Workstation - 8 Core, Dual GPU)    : Score = ", w_host, " | Power Share = ", share_host, "%");
    print("--------------------------------------------------------------------------------");
    print("Total Cluster Hardware Score               : ", total_weight);
    print("Enforcement Rule                           : Tasks dynamically scheduled by node weight.");
    print("================================================================================");
}
