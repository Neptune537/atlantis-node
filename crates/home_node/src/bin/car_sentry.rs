use home_node::car_sentry::{PdmHardwareInterface, VehicleSentryPacket};
use atlantis_protocol::PearlEnclaveIdentity;
use std::time::Duration;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let vehicle_id = std::env::var("VEHICLE_ID").unwrap_or_else(|_| "kuroshio-unit-01".to_string());
    info!("Starting Atlantis Car Sentry Daemon for {}", vehicle_id);

    // Initialize vehicle Ed25519 Pearl Enclave identity
    let identity = PearlEnclaveIdentity::from_secure_env("VEHICLE_SECRET_KEY", &vehicle_id);
    info!("Vehicle Identity DID: {}", identity.did());

    let mut pdm = PdmHardwareInterface::new();

    // Sentry event loop
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    let simulated_voltage = 12.6f32;

    loop {
        ticker.tick().await;

        // In production on the Pi: read optocoupler GPIOs (door, shock, ACC) and battery voltage
        let trigger = pdm.evaluate_triggers(simulated_voltage);

        if let Some(alarm) = trigger {
            warn!("ALARM TRIGGERED: {:?}", alarm);
            
            // Build and cryptographically sign sentry packet
            let packet = VehicleSentryPacket::create_signed(
                &identity,
                &vehicle_id,
                simulated_voltage,
                pdm.pin_acc_ignition,
                (41.8781, -87.6298), // GPS coordinates from gpsd
                0.0,
                Some(alarm),
            )?;

            info!(
                "Signed packet ready for LoRa RF (915 MHz) / DTN broadcast: {} (sig: {}...)",
                packet.vehicle_id,
                &packet.signature[..16]
            );
        } else {
            info!(
                "Heartbeat: Battery {:.2}V | Ignition: {} | Relays: Siren={}, Kill={}",
                simulated_voltage,
                pdm.pin_acc_ignition,
                pdm.relay_siren_active,
                pdm.relay_starter_kill_active
            );
        }
    }
}
