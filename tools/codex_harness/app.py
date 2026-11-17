#!/usr/bin/env python3
"""
Kuroshio Codex Studio - Visual GUI Dashboard
Runs locally at http://127.0.0.1:5050
Features:
- Dark cyber aesthetic matching Lemuria & Nautilus
- Model selector (gpt-4o, o1-preview, etc.)
- Interactive Prompt & Response panes
- System prompt tuning for Goose/Rust invariants
- Real-time token usage display
"""

import os
import json
import urllib.parse
from http.server import HTTPServer, BaseHTTPRequestHandler
from codex_agent_harness import CodexHarness

PORT = 5050

HTML_PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kuroshio Codex Studio // ChatGPT Harness</title>
    <style>
        :root {
            --bg: #030712;
            --surface: #0f172a;
            --panel: #1e293b;
            --accent: #38bdf8;
            --accent-glow: rgba(56, 189, 248, 0.2);
            --text: #f8fafc;
            --muted: #94a3b8;
            --border: #334155;
            --code-bg: #020617;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace; }
        body { background: var(--bg); color: var(--text); padding: 24px; min-height: 100vh; }
        .header { max-width: 1300px; margin: 0 auto 20px; display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border); padding-bottom: 16px; }
        .brand { display: flex; align-items: center; gap: 12px; }
        .logo { width: 36px; height: 36px; background: linear-gradient(135deg, #0284c7, #38bdf8); border-radius: 8px; display: flex; align-items: center; justify-content: center; font-weight: 800; font-size: 1.1rem; color: #fff; }
        .title { font-size: 1.3rem; font-weight: 700; letter-spacing: 0.05em; }
        .sub { font-size: 0.75rem; color: var(--muted); text-transform: uppercase; }
        
        .badge { background: var(--accent-glow); border: 1px solid var(--accent); color: var(--accent); padding: 4px 12px; border-radius: 9999px; font-size: 0.8rem; font-weight: 600; }
        
        .layout { max-width: 1300px; margin: 0 auto; display: grid; grid-template-columns: 320px 1fr; gap: 20px; }
        @media(max-width: 900px) { .layout { grid-template-columns: 1fr; } }
        
        .sidebar { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 18px; display: flex; flex-direction: column; gap: 16px; height: fit-content; }
        .label { font-size: 0.75rem; font-weight: 700; text-transform: uppercase; color: var(--accent); margin-bottom: 6px; }
        input, select, textarea { width: 100%; background: var(--code-bg); border: 1px solid var(--border); border-radius: 6px; padding: 10px; color: #fff; font-size: 0.85rem; outline: none; transition: 0.2s; }
        input:focus, select:focus, textarea:focus { border-color: var(--accent); }
        
        .main-stage { display: flex; flex-direction: column; gap: 16px; }
        .editor-card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 18px; display: flex; flex-direction: column; gap: 10px; }
        .editor-header { display: flex; justify-content: space-between; align-items: center; }
        
        .run-btn { background: #0284c7; color: #fff; border: none; border-radius: 6px; padding: 10px 20px; font-weight: 700; cursor: pointer; transition: 0.2s; display: flex; align-items: center; gap: 8px; font-size: 0.9rem; align-self: flex-end; }
        .run-btn:hover { background: #38bdf8; color: #020617; }
        
        .output-box { min-height: 280px; max-height: 500px; overflow-y: auto; background: var(--code-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px; font-family: "Fira Code", monospace; font-size: 0.85rem; line-height: 1.5; white-space: pre-wrap; color: #e2e8f0; }
        .status-pill { font-size: 0.75rem; color: var(--muted); }
    </style>
</head>
<body>
    <div class="header">
        <div class="brand">
            <div class="logo">CX</div>
            <div>
                <div class="title">KUROSHIO CODEX STUDIO</div>
                <div class="sub">Autonomous ChatGPT & OpenAI Engine Harness</div>
            </div>
        </div>
        <div class="badge">● LOCAL ENGINE ONLINE</div>
    </div>

    <div class="layout">
        <div class="sidebar">
            <div>
                <div class="label">OpenAI API Key</div>
                <input type="password" id="apiKey" placeholder="sk-proj-... (or leave empty if in env)">
                <div style="font-size:0.7rem; color:var(--muted); margin-top:4px;">Stays strictly in your local browser session.</div>
            </div>

            <div>
                <div class="label">Target Model</div>
                <select id="modelSelect">
                    <option value="gpt-4o" selected>gpt-4o (High Speed & Code Logic)</option>
                    <option value="o1-preview">o1-preview (Deep Multi-Step Reasoning)</option>
                    <option value="o1-mini">o1-mini (Compact & Fast Math/Code)</option>
                    <option value="gpt-4-turbo">gpt-4-turbo</option>
                </select>
            </div>

            <div>
                <div class="label">System Invariant Directive</div>
                <textarea id="systemPrompt" rows="6">You are the Kuroshio Engineering Codex Agent. 
You write high-performance Rust and Goose code adhering strictly to zero-GC memory models, 64-bit WGS84 ECEF coordinates, and FIPS compliance.</textarea>
            </div>

            <div style="border-top:1px solid var(--border); padding-top:12px;">
                <div class="label">Target Node Environment</div>
                <div style="font-size:0.75rem; color:var(--muted); line-height:1.5;">
                    • Workstation: Intel DG1 Iris Xe MAX<br>
                    • Seed Pi: 10.0.0.58 (Siren RF & Consensus)<br>
                    • Screen Pi: 10.0.0.60 (Nautilus Display)
                </div>
            </div>
        </div>

        <div class="main-stage">
            <div class="editor-card">
                <div class="editor-header">
                    <div class="label">Prompt / Code Generation Directive</div>
                    <span class="status-pill" id="statusPill">Ready</span>
                </div>
                <textarea id="userPrompt" rows="4" placeholder="e.g. Generate a zero-GC Goose struct for audio DSP buffer pooling, or write a FIPS compliance test..."></textarea>
                <button class="run-btn" id="runBtn" onclick="submitCodex()">
                    <span>Execute Directive &rarr;</span>
                </button>
            </div>

            <div class="editor-card">
                <div class="editor-header">
                    <div class="label">Generated Output & Inspection Console</div>
                    <button style="background:transparent; border:1px solid var(--border); color:var(--muted); padding:4px 8px; border-radius:4px; font-size:0.75rem; cursor:pointer;" onclick="copyOutput()">Copy Output</button>
                </div>
                <div class="output-box" id="outputBox">// Output will appear here...</div>
            </div>
        </div>
    </div>

    <script>
        async function submitCodex() {
            const prompt = document.getElementById("userPrompt").value.trim();
            if (!prompt) {
                alert("Please enter a directive prompt first.");
                return;
            }

            const apiKey = document.getElementById("apiKey").value.trim();
            const model = document.getElementById("modelSelect").value;
            const system = document.getElementById("systemPrompt").value.trim();

            const status = document.getElementById("statusPill");
            const btn = document.getElementById("runBtn");
            const out = document.getElementById("outputBox");

            status.innerText = "Querying Codex Engine...";
            btn.disabled = true;
            btn.style.opacity = 0.5;
            out.innerText = "// Generating code response from OpenAI API...";

            try {
                const resp = await fetch("/api/generate", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ prompt, api_key: apiKey, model, system })
                });
                const data = await resp.json();
                
                if (data.status === "SUCCESS") {
                    out.innerText = data.content;
                    status.innerText = `Complete (${data.usage.total_tokens || 0} tokens)`;
                } else if (data.status === "CONFIG_REQUIRED") {
                    out.innerText = "CONFIG REQUIRED: Please enter your OpenAI API key in the left sidebar or set OPENAI_API_KEY in terminal.";
                    status.innerText = "Key Missing";
                } else {
                    out.innerText = "ERROR: " + JSON.stringify(data, null, 2);
                    status.innerText = "Failed";
                }
            } catch(e) {
                out.innerText = "Network Error: " + e;
                status.innerText = "Error";
            } finally {
                btn.disabled = false;
                btn.style.opacity = 1.0;
            }
        }

        function copyOutput() {
            const text = document.getElementById("outputBox").innerText;
            navigator.clipboard.writeText(text);
            alert("Copied to clipboard!");
        }
    </script>
</body>
</html>"""

class CodexHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path in ["/", "/index.html"]:
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(HTML_PAGE.encode("utf-8"))))
            self.end_headers()
            self.wfile.write(HTML_PAGE.encode("utf-8"))
            return
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        if self.path == "/api/generate":
            content_len = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_len)
            req_data = json.loads(body.decode("utf-8"))

            prompt = req_data.get("prompt", "")
            api_key = req_data.get("api_key") or None
            model = req_data.get("model", "gpt-4o")
            system = req_data.get("system", "")

            harness = CodexHarness(api_key=api_key, model=model)
            res = harness.send_prompt(system, prompt)

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(res).encode("utf-8"))
            return

        self.send_response(404)
        self.end_headers()

if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", PORT), CodexHandler)
    print(f"[*] Kuroshio Codex Studio GUI running on http://127.0.0.1:{PORT}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
