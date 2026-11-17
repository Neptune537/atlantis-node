#!/usr/bin/env python3
"""
Interactive CLI for the Kuroshio Codex Harness
Usage:
  python3 cli.py "Generate a Rust struct for a tactical radar blip in Lemuria"
"""

import sys
from codex_agent_harness import CodexHarness

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 cli.py \"<prompt or task directive>\"")
        sys.exit(1)

    prompt = " ".join(sys.argv[1:])
    harness = CodexHarness()
    
    system_prompt = (
        "You are the Kuroshio Engineering Codex Agent. "
        "You write idiomatic, high-performance Rust and Goose code adhering strictly to "
        "zero-GC memory models, 64-bit WGS84 ECEF coordinates, and FIPS-compliant cryptographic invariants."
    )

    print(f"[*] Submitting prompt to Codex Harness ({harness.model})...")
    res = harness.send_prompt(system_prompt, prompt)

    if res.get("status") == "SUCCESS":
        print("\n=== CODEX HARNESS RESPONSE ===")
        print(res["content"])
    elif res.get("status") == "CONFIG_REQUIRED":
        print(f"\n[!] {res['message']}")
        print("    Run: export OPENAI_API_KEY='sk-...' to activate live OpenAI generation.")
    else:
        print(f"\n[!] Error from harness: {res}")

if __name__ == "__main__":
    main()
