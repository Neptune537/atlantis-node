use home_node::ClubHomeNode;
use std::time::Duration;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let node_id = std::env::var("ATLANTIS_NODE_ID").unwrap_or_else(|_| "zero-rpi-node-01".to_string());
    let dividend_rate: f64 = std::env::var("ATLANTIS_DIVIDEND_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.02);

    info!("Starting Atlantis Club Home Node daemon...");
    info!("Node ID: {}", node_id);
    info!("Treasury Dividend Rate: {:.2}%", dividend_rate * 100.0);

    let home_node = ClubHomeNode::new(&node_id, dividend_rate);
    info!("Treasury account seeded: {}", home_node.club_treasury_account);
    info!("OpenRaft consensus initialized. Current role: {:?}", home_node.consensus.role);
    info!("Local AI Gate status: ready");
    info!("DTN store active: WAN status = {}", home_node.dtn_store.is_wan_online);
    info!("Atlantis Home Node daemon running and listening for Kuroshio Gyre tasks.");

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                info!(
                    "Heartbeat: Node {} healthy | Raft Term: {} | Committed Index: {} | DTN Queue: {}",
                    home_node.club_node_id,
                    home_node.consensus.current_term,
                    home_node.consensus.committed_log_index,
                    home_node.dtn_store.buffered_count()
                );
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT/SIGTERM, shutting down Atlantis Home Node cleanly...");
                break;
            }
        }
    }

    Ok(())
}
