//! Native benchmark of the actual OracleHost, using synthetic signed challenges.
//! Each wave contains independent chains; elapsed includes signing and queue wait.
use ed25519_dalek::{Signer, SigningKey};
use kuroshio_mesh::{
    oracle_verification::{OraclePolicy, Resolution},
    temporal_anchor::{Canary, OracleHost},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn signed(host: &mut OracleHost, key: &SigningKey) -> Canary {
    let challenge = host
        .challenge(
            [9; 32],
            OraclePolicy {
                non_critical: false,
                budget: Duration::from_millis(250),
            },
            Duration::ZERO,
        )
        .unwrap();
    let mut canary = Canary {
        challenge,
        iterations: 4096,
        digest: [0; 32],
        signature: vec![],
    };
    canary.digest = canary.compute_digest().unwrap();
    canary.signature = key.sign(&canary.signing_bytes()).to_bytes().to_vec();
    canary
}
fn host(key: &SigningKey) -> OracleHost {
    OracleHost::new(key.verifying_key(), 1, 0, [0; 32])
}
fn proc_field(path: &str, name: &str) -> Option<u64> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find(|line| line.starts_with(name))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn temperature() -> Option<f64> {
    fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|v| v / 1000.0)
}
fn throttle() -> Option<String> {
    Command::new("vcgencmd")
        .arg("get_throttled")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
fn stress(mode: &str, file: PathBuf) -> ! {
    let bytes = vec![1_u8; 1024 * 1024];
    loop {
        match mode {
            "cpu" => {
                std::hint::black_box(Sha256::digest(&bytes));
            }
            "io" => {
                let mut output = fs::File::create(&file).unwrap();
                for _ in 0..4 {
                    output.write_all(&bytes).unwrap();
                }
                output.sync_all().unwrap();
                thread::sleep(Duration::from_millis(50));
            }
            "audio" => {
                let pcm: Vec<i16> = (0..4410)
                    .map(|i| {
                        (1000.0
                            * (2.0 * std::f64::consts::PI * 432.0 * f64::from(i) / 44100.0).sin())
                            as i16
                    })
                    .collect();
                std::hint::black_box(pcm);
                thread::sleep(Duration::from_millis(100));
            }
            _ => std::process::exit(1),
        }
    }
}
struct Load {
    child: Child,
    file: PathBuf,
}
impl Drop for Load {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.file);
    }
}
fn percentile(values: &[f64], p: f64) -> f64 {
    values[((values.len() as f64 * p).ceil() as usize).saturating_sub(1)]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--stress") {
        stress(
            args.get(2).ok_or("mode missing")?,
            args.get(3).ok_or("file missing")?.into(),
        );
    }
    let wave: usize = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(8);
    if !(1..=32).contains(&wave) {
        return Err("wave must be 1..32".into());
    }
    let key = SigningKey::from_bytes(&[7; 32]); // Synthetic, never a production authority.
    let mut modes: Vec<Value> = Vec::new();
    for mode in ["idle", "cpu", "io", "audio"] {
        let file =
            std::env::temp_dir().join(format!("nautilus-native-{}-{mode}.tmp", std::process::id()));
        let load = if mode == "idle" {
            None
        } else {
            Some(Load {
                child: Command::new(std::env::current_exe()?)
                    .args(["--stress", mode])
                    .arg(&file)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?,
                file,
            })
        };
        let start_temp = temperature();
        let start_throttle = throttle();
        let started = Instant::now();
        let mut times = Vec::new();
        let mut misses = 0;
        while times.len() < 128 {
            let count = wave.min(128 - times.len());
            let mut queued = Vec::new();
            for _ in 0..count {
                let mut oracle = host(&key);
                let canary = signed(&mut oracle, &key);
                queued.push((oracle, canary));
            }
            for (mut oracle, canary) in queued {
                let verdict = oracle.verify(canary)?;
                times.push(verdict.elapsed.as_secs_f64() * 1000.0);
                if verdict.resolution != Resolution::Anchored {
                    misses += 1;
                }
            }
        }
        let mut oracle = host(&key);
        let mut invalid = signed(&mut oracle, &key);
        invalid.signature = vec![0; 64];
        let invalid_rejected = oracle.verify(invalid).is_err();
        let mut bad = signed(&mut oracle, &key);
        bad.digest = [0; 32];
        bad.signature = key.sign(&bad.signing_bytes()).to_bytes().to_vec();
        let bad_digest_rejected = oracle.verify(bad).is_err();
        let good = signed(&mut oracle, &key);
        let recovery = oracle.verify(good.clone())?;
        let replay_rejected = oracle.verify(good).is_err();
        if !invalid_rejected || !bad_digest_rejected || !replay_rejected {
            return Err("invalid evidence accepted".into());
        }
        times.sort_by(f64::total_cmp);
        modes.push(json!({"mode":mode,"samples":times.len(),"p50_ms":percentile(&times,0.5),"p95_ms":percentile(&times,0.95),"p99_ms":percentile(&times,0.99),"max_ms":times.last(),"deadline_misses_250ms":misses,
            "wall_seconds":started.elapsed().as_secs_f64(),"peak_rss_kib":proc_field("/proc/self/status","VmHWM:"),"temperature_c_start":start_temp,"temperature_c_end":temperature(),"throttled_start":start_throttle,"throttled_end":throttle(),
            "invalid_signature_rejected":invalid_rejected,"bad_digest_rejected":bad_digest_rejected,"replay_rejected":replay_rejected,"recovery_ms":recovery.elapsed.as_secs_f64()*1000.0}));
        drop(load);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"scope":"native Rust OracleHost signed synthetic challenge verification","architecture":std::env::consts::ARCH,"iterations":4096,"independent_chain_queue_wave":wave,"results":modes,"rust_host_tested":true,"clock_uncertainty_measured":false,"reticulum_transport_tested":false,"wasmtime_tested":false,"hardware_acceptance":false})
        )?
    );
    Ok(())
}
