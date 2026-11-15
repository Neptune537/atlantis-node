use ed25519_dalek::{Signer, SigningKey};
use kuroshio_mesh::temporal_anchor::{Canary, Challenge};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};
use uuid::Uuid;

#[test]
fn stdio_signed_evidence_and_replay_roundtrip() {
    let key = SigningKey::from_bytes(&[7; 32]);
    let path = std::env::temp_dir().join(format!("nautilus-mcp-{}.json", Uuid::new_v4()));
    std::fs::write(
        &path,
        json!({"authority_key":key.verifying_key().to_bytes(),"epoch":1,"sequence":0,
        "checkpoint":vec![0;32],"uncertainty_ms":0})
        .to_string(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_oracle_mcp"))
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut input = child.stdin.take().unwrap();
    let mut rpc = |message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert_eq!(
        rpc(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
        )["result"]["protocolVersion"],
        "2025-11-25"
    );
    let response = rpc(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"oracle_challenge","arguments":{"tile":vec![9;32],"visual_only":true}}}),
    );
    let content: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let challenge: Challenge = serde_json::from_value(content["challenge"].clone()).unwrap();
    let mut canary = Canary {
        challenge,
        iterations: 2,
        digest: [0; 32],
        signature: vec![],
    };
    canary.digest = canary.compute_digest().unwrap();
    canary.signature = key.sign(&canary.signing_bytes()).to_bytes().to_vec();
    let request = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"oracle_verification","arguments":{"canary":canary}}});
    let response = rpc(request.clone());
    assert_eq!(response["result"]["isError"], false);
    let content: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["resolution"], "VisualAnchored");
    assert_eq!(content["settlement_authorized"], false);
    assert_eq!(rpc(request)["result"]["isError"], true);
    drop(rpc);
    drop(input);
    assert!(child.wait().unwrap().success());
    std::fs::remove_file(path).unwrap();
}
