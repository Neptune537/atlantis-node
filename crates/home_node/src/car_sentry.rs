use atlantis_protocol::{AtlantisProtocolError, PearlEnclaveIdentity};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::time::{SystemTime, UNIX_EPOCH};

/// Physical trigger alerts detected by optocouplers, OBD-II, or software sentry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehicleAlarmTrigger {
    DoorBreached,
    ShockDetected,
    TiltTowingDetected,
    GlassBreakAcoustic,
    IgnitionTamper,
    LowBatteryVoltage,
    PanicButton,
    SentryMotionDetected,
}

/// Compact security telemetry frame broadcasted over SX1262 LoRa (915 MHz) or saved to DTN spool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VehicleSentryPacket {
    pub vehicle_id: String,
    pub timestamp_epoch_sec: u64,
    pub battery_voltage: f32,
    pub ignition_on: bool,
    pub lat_lon: (f64, f64),
    pub speed_kmh: f32,
    pub trigger: Option<VehicleAlarmTrigger>,
    pub signature: String,
}

impl VehicleSentryPacket {
    /// Creates and cryptographically signs a sentry packet with the vehicle's Pearl Enclave DID.
    pub fn create_signed(
        identity: &PearlEnclaveIdentity,
        vehicle_id: impl Into<String>,
        battery_voltage: f32,
        ignition_on: bool,
        lat_lon: (f64, f64),
        speed_kmh: f32,
        trigger: Option<VehicleAlarmTrigger>,
    ) -> Result<Self, AtlantisProtocolError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let vid = vehicle_id.into();
        let payload = format!(
            "{}:{}:{:.2}:{}:{:.5}:{:.5}:{:.1}:{:?}",
            vid, ts, battery_voltage, ignition_on, lat_lon.0, lat_lon.1, speed_kmh, trigger
        );

        let digest = sha2::Sha256::digest(payload.as_bytes());
        let sig_bytes = identity.sign_raw_digest(&digest.into());
        let signature = atlantis_protocol::bytes_to_hex(&sig_bytes);

        Ok(Self {
            vehicle_id: vid,
            timestamp_epoch_sec: ts,
            battery_voltage,
            ignition_on,
            lat_lon,
            speed_kmh,
            trigger,
            signature,
        })
    }

    /// Verifies that the sentry packet signature matches the vehicle identity pubkey.
    pub fn verify_signature(&self, identity: &PearlEnclaveIdentity) -> Result<bool, AtlantisProtocolError> {
        let payload = format!(
            "{}:{}:{:.2}:{}:{:.5}:{:.5}:{:.1}:{:?}",
            self.vehicle_id,
            self.timestamp_epoch_sec,
            self.battery_voltage,
            self.ignition_on,
            self.lat_lon.0,
            self.lat_lon.1,
            self.speed_kmh,
            self.trigger
        );

        let digest = sha2::Sha256::digest(payload.as_bytes());
        Ok(PearlEnclaveIdentity::verify_raw_digest(&identity.did(), &digest.into(), &self.signature))
    }
}

/// Simulated / hardware interface for PDM GPIO lines.
#[derive(Debug, Clone, Default)]
pub struct PdmHardwareInterface {
    pub pin_acc_ignition: bool,
    pub pin_door_sensor: bool,
    pub pin_shock_sensor: bool,
    pub relay_siren_active: bool,
    pub relay_starter_kill_active: bool,
}

impl PdmHardwareInterface {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates hardware lines to detect alarm conditions.
    pub fn evaluate_triggers(&mut self, voltage: f32) -> Option<VehicleAlarmTrigger> {
        if voltage < 11.8 {
            return Some(VehicleAlarmTrigger::LowBatteryVoltage);
        }
        if self.pin_shock_sensor {
            self.relay_siren_active = true;
            return Some(VehicleAlarmTrigger::ShockDetected);
        }
        if self.pin_door_sensor && !self.pin_acc_ignition {
            self.relay_siren_active = true;
            return Some(VehicleAlarmTrigger::DoorBreached);
        }
        None
    }

    /// Engages or disengages anti-theft starter-kill relay.
    pub fn set_starter_kill(&mut self, kill: bool) {
        self.relay_starter_kill_active = kill;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_car_sentry_packet_signature_and_verification() {
        let identity = PearlEnclaveIdentity::generate_ephemeral();
        let packet = VehicleSentryPacket::create_signed(
            &identity,
            "vehicle_vin_9921",
            12.6,
            false,
            (41.8781, -87.6298),
            0.0,
            Some(VehicleAlarmTrigger::DoorBreached),
        )
        .expect("packet signed");

        assert!(packet.verify_signature(&identity).expect("verified"));
    }

    #[test]
    fn test_pdm_alarm_evaluation_and_relays() {
        let mut pdm = PdmHardwareInterface::new();
        assert_eq!(pdm.evaluate_triggers(12.4), None);

        // Door opened while ignition off
        pdm.pin_door_sensor = true;
        let trigger = pdm.evaluate_triggers(12.4);
        assert_eq!(trigger, Some(VehicleAlarmTrigger::DoorBreached));
        assert!(pdm.relay_siren_active);

        // Low voltage trigger (XH-M609 cutoff threshold)
        pdm.pin_door_sensor = false;
        let low_v = pdm.evaluate_triggers(11.6);
        assert_eq!(low_v, Some(VehicleAlarmTrigger::LowBatteryVoltage));
    }
}
