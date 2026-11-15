//! Local stdio MCP adapter pinned to legacy protocol 2025-11-25.
//! Diagnostic verification only: no network access, signing keys or ledger writes.
use ed25519_dalek::VerifyingKey;
use kuroshio_mesh::{
    oracle_verification::OraclePolicy,
    temporal_anchor::{Canary, OracleHost},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    authority_key: [u8; 32],
    epoch: u64,
    sequence: u64,
    checkpoint: [u8; 32],
    uncertainty_ms: u64,
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}
fn tile_schema() -> Value {
    json!({"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":32,"maxItems":32})
}
fn tool_result(id: Value, result: Result<Value, String>) -> Value {
    let failed = result.is_err();
    let value = result.unwrap_or_else(|message| json!({"error": message}));
    json!({"jsonrpc":"2.0","id":id,"result":{"isError":failed,"content":[{"type":"text","text":value.to_string()}]}})
}

fn handle(
    message: Value,
    initialized: &mut bool,
    host: &mut OracleHost,
    uncertainty: Duration,
) -> Option<Value> {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if message.get("jsonrpc") != Some(&json!("2.0")) {
        return Some(error(id, -32600, "Invalid JSON-RPC request"));
    }
    if !message.get("id").is_some() {
        return None;
    }
    if !(id.is_number() || id.is_string()) {
        return Some(error(Value::Null, -32600, "Invalid request ID"));
    }
    if method == "initialize" {
        if *initialized {
            return Some(error(id, -32600, "Already initialized"));
        }
        *initialized = true;
        return Some(
            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-11-25",
            "capabilities":{"tools":{}},"serverInfo":{"name":"nautilus-oracle","version":"0.1.0"}}}),
        );
    }
    if method == "server/discover" {
        return Some(error(
            id,
            -32601,
            "Legacy MCP 2025-11-25: initialize required",
        ));
    }
    if !*initialized {
        return Some(error(id, -32600, "Initialize first (MCP 2025-11-25)"));
    }
    match method {
        "ping" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "tools/list" => Some(json!({"jsonrpc":"2.0","id":id,"result":{"tools":[
            {"name":"oracle_challenge","description":"Issue a synthetic/coarse tile challenge. Critical deadline is 250ms; visual mode cannot authorize settlement.",
             "inputSchema":{"type":"object","properties":{"tile":tile_schema(),"visual_only":{"type":"boolean"}},"required":["tile","visual_only"],"additionalProperties":false}},
            {"name":"oracle_verification","description":"Verify signed challenge-bound evidence. Diagnostic result only; never a transferable settlement authorization.",
             "inputSchema":{"type":"object","properties":{"canary":{"type":"object"}},"required":["canary"],"additionalProperties":false}}
        ]}})),
        "tools/call" => {
            let name = message["params"]["name"].as_str().unwrap_or("");
            let args = &message["params"]["arguments"];
            let result = match name {
                "oracle_challenge" => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Args {
                        tile: [u8; 32],
                        visual_only: bool,
                    }
                    serde_json::from_value::<Args>(args.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|args| {
                            host.challenge(
                                args.tile,
                                OraclePolicy {
                                    non_critical: args.visual_only,
                                    budget: Duration::from_millis(if args.visual_only {
                                        1000
                                    } else {
                                        250
                                    }),
                                },
                                uncertainty,
                            )
                            .map(|c| json!({"challenge":c}))
                            .map_err(|e| e.to_string())
                        })
                }
                "oracle_verification" => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Args {
                        canary: Canary,
                    }
                    serde_json::from_value::<Args>(args.clone()).map_err(|e| e.to_string()).and_then(|args| {
                        host.verify(args.canary).map(|verdict| json!({"resolution":format!("{:?}",verdict.resolution),
                            "elapsed_ms":verdict.elapsed.as_secs_f64()*1000.0,"settlement_authorized":false}))
                            .map_err(|e| e.to_string())
                    })
                }
                _ => return Some(error(id, -32602, "Unknown tool")),
            };
            Some(tool_result(id, result))
        }
        _ => Some(error(id, -32601, "Method not found")),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: oracle_mcp TRUSTED_CONFIG.json")?;
    let config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut host = OracleHost::new(
        VerifyingKey::from_bytes(&config.authority_key)?,
        config.epoch,
        config.sequence,
        config.checkpoint,
    );
    let mut initialized = false;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        // Bound allocation even when a client never sends a newline.
        let read = input
            .by_ref()
            .take(16 * 1024 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > 16 * 1024 {
            return Err("MCP input exceeds 16 KiB".into());
        }
        let response = match serde_json::from_slice::<Value>(&line) {
            Ok(message) => handle(
                message,
                &mut initialized,
                &mut host,
                Duration::from_millis(config.uncertainty_ms),
            ),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    #[test]
    fn initialize_list_challenge_invalid_and_notifications() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut host = OracleHost::new(key.verifying_key(), 1, 0, [0; 32]);
        let mut initialized = false;
        let mut call = |message| handle(message, &mut initialized, &mut host, Duration::ZERO);
        assert!(call(json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}))
            .unwrap()
            .get("error")
            .is_some());
        assert_eq!(
            call(json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap()["result"]
                ["protocolVersion"],
            "2025-11-25"
        );
        assert!(call(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        assert_eq!(
            call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap()["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let response = call(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"oracle_challenge","arguments":{"tile":vec![0;32],"visual_only":true}}})).unwrap();
        assert_eq!(response["result"]["isError"], false);
        let response = call(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"oracle_verification","arguments":{}}})).unwrap();
        assert_eq!(response["result"]["isError"], true);
    }
}
