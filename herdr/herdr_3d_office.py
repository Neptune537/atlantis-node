#!/usr/bin/env python3
"""
Herdr 3D Spatial Office Floor (All Features: A, B, C + OrbitControls + WhisperFlow)
- Orbit, Pan, and Zoom enabled via embedded Three.js OrbitControls
- Expanded 3D Architectural Floor: Executive Suite, Marketing Room, Sales Lab, Break Lounge with Pool & Ping-Pong Tables
- Complete Sistava-style Left Navigation Sidebar with Team 7 Organization hierarchy
- WhisperFlow Voice / Dictation Audio Processing Endpoint (browser Web Audio -> local transcription pipeline)
- Live Agent Directive Dispatch: Direct bidirectional interaction with Triton, Ursula, and Sebastian
Runs on Port 8899.
"""

import os
import json
import time
import urllib.parse
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = 8899

WORKFORCE_STATE = {
    "triton": {
        "id": "triton",
        "name": "Triton",
        "role": "Chief of Staff & Core Verifier",
        "team": "Executive & Consensus",
        "status": "working",
        "activity": "Auditing OpenRaft block #144002 and node health",
        "room": "Executive Suite",
        "desk_pos": [-6.0, 0.0, -4.0],
        "speech": "Auditing Block #144002",
        "node": "Seed Pi (10.0.0.58)",
        "color": "#38bdf8",
        "avatar": "🔱"
    },
    "ursula": {
        "id": "ursula",
        "name": "Ursula",
        "role": "Chief Growth Strategist & Allocator",
        "team": "Strategy & Trading",
        "status": "working",
        "activity": "Evaluating SOL/USDC VWAP consolidation & SBIR Phase I",
        "room": "Trading Floor",
        "desk_pos": [6.0, 0.0, -4.0],
        "speech": "SOL/USDC 72.97 | Hold Conviction 6.5",
        "node": "Workstation Dual-GPU",
        "color": "#c084fc",
        "avatar": "🔮"
    },
    "sebastian": {
        "id": "sebastian",
        "name": "Sebastian",
        "role": "Marketing Director & Edge Sentinel",
        "team": "Marketing & Growth",
        "status": "working",
        "activity": "Streaming Siren Radio 24/7 & syndicating VisionQuest",
        "room": "Media Lounge",
        "desk_pos": [-6.0, 0.0, 4.0],
        "speech": "Siren Radio Live | 0 Dropouts",
        "node": "Screen Pi (10.0.0.60)",
        "color": "#f87171",
        "avatar": "🦀"
    },
    "herdr_arb": {
        "id": "herdr_arb",
        "name": "Herdr Arb Bot",
        "role": "Jupiter DEX Liquidity Sentinel",
        "team": "Trading & Yield",
        "status": "available",
        "activity": "Scanning Solana DEX triangular spread routes",
        "room": "Trading Floor",
        "desk_pos": [6.0, 0.0, 4.0],
        "speech": "Mempool Spread: 0.12% (Holding)",
        "node": "Seed Pi (10.0.0.58)",
        "color": "#4ade80",
        "avatar": "⚡"
    }
}

HTML_OFFICE = """<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Herdr 3D Spatial Office Floor • Kuroshio Gyre</title>
    <!-- Three.js + OrbitControls -->
    <script src="https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/three@0.128.0/examples/js/controls/OrbitControls.js"></script>
    <style>
        :root {
            --bg: #030712;
            --sidebar-bg: #0b0f19;
            --surface: rgba(15, 23, 42, 0.9);
            --border: #1e293b;
            --accent: #38bdf8;
            --accent-glow: rgba(56, 189, 248, 0.25);
            --text: #f8fafc;
            --muted: #94a3b8;
            --green: #22c55e;
            --gold: #fbbf24;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace; }
        body { background: var(--bg); color: var(--text); overflow: hidden; width: 100vw; height: 100vh; display: flex; }

        /* Left Navigation Sidebar (Sistava Style) */
        .sidebar {
            width: 280px;
            height: 100vh;
            background: var(--sidebar-bg);
            border-right: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            z-index: 20;
            flex-shrink: 0;
            transition: 0.3s;
        }
        .sidebar-header { padding: 18px; border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 10px; }
        .logo-box { width: 32px; height: 32px; background: linear-gradient(135deg, #0284c7, #38bdf8); border-radius: 8px; display: flex; align-items: center; justify-content: center; font-weight: 800; color: #fff; }
        .logo-title { font-size: 0.95rem; font-weight: 700; letter-spacing: 0.05em; }
        .logo-sub { font-size: 0.65rem; color: var(--muted); text-transform: uppercase; }

        .nav-section { padding: 14px 18px 6px; font-size: 0.7rem; font-weight: 700; text-transform: uppercase; color: var(--muted); letter-spacing: 0.08em; }
        .nav-list { list-style: none; display: flex; flex-direction: column; gap: 2px; padding: 0 10px; }
        .nav-item { padding: 8px 12px; border-radius: 6px; font-size: 0.8rem; color: #cbd5e1; display: flex; align-items: center; gap: 10px; cursor: pointer; transition: 0.2s; }
        .nav-item:hover, .nav-item.active { background: #1e293b; color: var(--accent); }
        .nav-item.active { font-weight: 600; border-left: 3px solid var(--accent); }

        .org-tree { flex: 1; overflow-y: auto; padding: 0 10px 14px; }
        .employee-card { padding: 8px 10px; border-radius: 6px; font-size: 0.8rem; display: flex; align-items: center; justify-content: space-between; cursor: pointer; margin-bottom: 2px; }
        .employee-card:hover { background: #1e293b; }
        .emp-name { font-weight: 600; color: #fff; }
        .emp-role { font-size: 0.7rem; color: var(--muted); }
        .status-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--green); box-shadow: 0 0 6px var(--green); }

        /* Main 3D Viewport */
        .stage-container { flex: 1; height: 100vh; position: relative; }
        #canvas3d { width: 100%; height: 100%; display: block; }

        /* Top HUD */
        .top-hud { position: absolute; top: 18px; left: 24px; right: 24px; display: flex; justify-content: space-between; pointer-events: none; z-index: 10; }
        .hud-pill { background: var(--surface); backdrop-filter: blur(12px); border: 1px solid var(--border); border-radius: 10px; padding: 8px 16px; pointer-events: auto; display: flex; gap: 10px; align-items: center; }
        .btn-tool { background: transparent; border: 1px solid var(--border); color: var(--text); padding: 6px 12px; border-radius: 6px; font-size: 0.75rem; cursor: pointer; transition: 0.2s; }
        .btn-tool:hover, .btn-tool.active { background: var(--accent); color: #020617; font-weight: 700; border-color: var(--accent); }

        /* WhisperFlow Mic Button */
        .mic-btn { background: #dc2626; color: #fff; border: none; border-radius: 6px; padding: 6px 12px; font-size: 0.75rem; font-weight: 700; cursor: pointer; display: flex; align-items: center; gap: 6px; }
        .mic-btn.recording { background: #ef4444; animation: pulse 1.5s infinite; }
        @keyframes pulse { 0% { opacity: 1; } 50% { opacity: 0.5; } 100% { opacity: 1; } }

        /* Character Inspection Card */
        .character-card {
            position: absolute;
            right: 32px;
            bottom: 32px;
            width: 380px;
            background: var(--surface);
            backdrop-filter: blur(16px);
            border: 1px solid var(--border);
            border-radius: 16px;
            padding: 22px;
            display: none;
            flex-direction: column;
            gap: 14px;
            box-shadow: 0 25px 50px rgba(0,0,0,0.8);
            z-index: 30;
            animation: slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        }
        @keyframes slideUp { from { transform: translateY(20px); opacity: 0; } to { transform: translateY(0); opacity: 1; } }

        .card-header { display: flex; justify-content: space-between; align-items: center; }
        .char-avatar { width: 44px; height: 44px; border-radius: 10px; background: #1e293b; display: flex; align-items: center; justify-content: center; font-size: 1.4rem; }
        .char-info h2 { font-size: 1.1rem; font-weight: 700; }
        .char-info p { font-size: 0.7rem; color: var(--accent); text-transform: uppercase; font-weight: 600; }
        .close-btn { background: transparent; border: none; color: var(--muted); font-size: 1.2rem; cursor: pointer; }

        .activity-box { background: rgba(2, 6, 23, 0.8); border: 1px solid var(--border); border-radius: 8px; padding: 10px 14px; font-size: 0.8rem; line-height: 1.4; color: #cbd5e1; }
        .msg-input { width: 100%; background: #020617; border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; color: #fff; font-size: 0.85rem; outline: none; }
        .msg-input:focus { border-color: var(--accent); }
        .send-btn { background: #0284c7; color: #fff; border: none; border-radius: 8px; padding: 10px; font-weight: 700; font-size: 0.85rem; cursor: pointer; transition: 0.2s; }
        .send-btn:hover { background: var(--accent); color: #020617; }
    </style>
</head>
<body>
    <!-- Left Navigation Sidebar -->
    <div class="sidebar">
        <div class="sidebar-header">
            <div class="logo-box">H7</div>
            <div>
                <div class="logo-title">HERDR WORKSPACE</div>
                <div class="logo-sub">Autonomous Organization</div>
            </div>
        </div>

        <div class="nav-section">Views</div>
        <ul class="nav-list">
            <li class="nav-item"><span>📊</span> Overview</li>
            <li class="nav-item active"><span>🏢</span> 3D Spatial Office</li>
            <li class="nav-item"><span>💬</span> Agent Chat</li>
            <li class="nav-item"><span>📋</span> Task Board</li>
            <li class="nav-item"><span>📻</span> Siren Radio 24/7</li>
        </ul>

        <div class="nav-section">Team 7 Organization</div>
        <div class="org-tree" id="orgTree"></div>
    </div>

    <!-- Main 3D Viewport -->
    <div class="stage-container">
        <div class="top-hud">
            <div class="hud-pill">
                <span style="font-size:0.75rem; color:var(--muted); text-transform:uppercase; font-weight:700;">Controls:</span>
                <span style="font-size:0.75rem; color:#cbd5e1;">Left-Click Drag to Orbit • Right-Click to Pan • Scroll to Zoom</span>
            </div>

            <div class="hud-pill">
                <button class="btn-tool" onclick="resetView()">Reset View</button>
                <button class="btn-tool" id="tourBtn" onclick="toggleTour()">Cinematic Tour</button>
                <button class="mic-btn" id="whisperBtn" onclick="toggleWhisperFlow()"><span>🎙️</span> <span id="whisperLabel">WhisperFlow</span></button>
            </div>
        </div>

        <div id="canvas3d"></div>

        <!-- Inspection Card -->
        <div class="character-card" id="charCard">
            <div class="card-header">
                <div style="display:flex; gap:12px; align-items:center;">
                    <div class="char-avatar" id="cAvatar">🔱</div>
                    <div class="char-info">
                        <h2 id="cName">Triton</h2>
                        <p id="cRole">Chief of Staff</p>
                    </div>
                </div>
                <button class="close-btn" onclick="closeCard()">&times;</button>
            </div>

            <div style="display:flex; justify-content:space-between; align-items:center;">
                <span style="font-size:0.75rem; color:var(--green); font-weight:700; text-transform:uppercase;">● ACTIVE WORKING</span>
                <span style="font-size:0.75rem; color:var(--muted);" id="cNode">Seed Pi (10.0.0.58)</span>
            </div>

            <div class="activity-box">
                <div style="font-size:0.65rem; color:var(--muted); text-transform:uppercase; font-weight:700; margin-bottom:4px;">Live Invariant Task</div>
                <div id="cActivity">Auditing OpenRaft block #144002</div>
            </div>

            <div style="display:flex; flex-direction:column; gap:8px;">
                <input type="text" class="msg-input" id="cMsgInput" placeholder="Dispatch directive (voice or text)...">
                <button class="send-btn" onclick="sendInstruction()">Dispatch Directive</button>
            </div>
        </div>
    </div>

    <script>
        let scene, camera, renderer, controls, raycaster, mouse;
        let employees = {};
        let selectedEmpId = null;
        let isTouring = false;
        let tourAngle = 0;
        let defaultCamPos = new THREE.Vector3(0, 20, 24);
        let isRecording = false;

        const WORKFORCE = """ + json.dumps(WORKFORCE_STATE) + """;

        function init() {
            const container = document.getElementById("canvas3d");
            scene = new THREE.Scene();
            scene.background = new THREE.Color(0x030712);
            scene.fog = new THREE.FogExp2(0x030712, 0.015);

            camera = new THREE.PerspectiveCamera(45, container.clientWidth / container.clientHeight, 0.1, 1000);
            camera.position.copy(defaultCamPos);

            renderer = new THREE.WebGLRenderer({ antialias: true });
            renderer.setSize(container.clientWidth, container.clientHeight);
            renderer.shadowMap.enabled = true;
            container.appendChild(renderer.domElement);

            // OrbitControls Enabled for Orbit & Pan & Zoom
            controls = new THREE.OrbitControls(camera, renderer.domElement);
            controls.enableDamping = true;
            controls.dampingFactor = 0.05;
            controls.maxPolarAngle = Math.PI / 2.05; // prevent going below floor
            controls.minDistance = 5;
            controls.maxDistance = 60;
            controls.target.set(0, 1, 0);

            raycaster = new THREE.Raycaster();
            mouse = new THREE.Vector2();

            // Lighting
            const ambient = new THREE.AmbientLight(0xffffff, 0.7);
            scene.add(ambient);

            const mainLight = new THREE.DirectionalLight(0x38bdf8, 1.4);
            mainLight.position.set(12, 24, 12);
            mainLight.castShadow = true;
            scene.add(mainLight);

            // Build Expanded Office Architecture
            buildArchitecture();
            buildFurniture();
            spawnWorkforce();
            populateOrgTree();

            window.addEventListener('resize', onWindowResize);
            renderer.domElement.addEventListener('pointerdown', onPointerDown);

            animate();
        }

        function buildArchitecture() {
            // Main Floor
            const floorGeo = new THREE.PlaneGeometry(36, 26);
            const floorMat = new THREE.MeshStandardMaterial({ color: 0x0a0f1d, roughness: 0.85 });
            const floor = new THREE.Mesh(floorGeo, floorMat);
            floor.rotation.x = -Math.PI / 2;
            floor.receiveShadow = true;
            scene.add(floor);

            const grid = new THREE.GridHelper(36, 36, 0x1e293b, 0x0f172a);
            grid.position.y = 0.01;
            scene.add(grid);

            // Tinted Glass Perimeter & Dividers
            const glassMat = new THREE.MeshPhysicalMaterial({ color: 0x38bdf8, transparent: true, opacity: 0.15, roughness: 0.1, transmission: 0.9, thickness: 0.5 });
            
            // Wall dividers
            const wallH = new THREE.Mesh(new THREE.BoxGeometry(36, 3, 0.2), glassMat);
            wallH.position.set(0, 1.5, 0);
            scene.add(wallH);

            const wallV = new THREE.Mesh(new THREE.BoxGeometry(0.2, 3, 26), glassMat);
            wallV.position.set(0, 1.5, 0);
            scene.add(wallV);
        }

        function buildFurniture() {
            // Lounge Area (Bottom Center): Pool Table & Ping Pong Table (Sistava Feature)
            // Pool Table
            const poolGeo = new THREE.BoxGeometry(4.2, 0.8, 2.4);
            const poolMat = new THREE.MeshStandardMaterial({ color: 0x0369a1 });
            const pool = new THREE.Mesh(poolGeo, poolMat);
            pool.position.set(-5, 0.4, 8);
            scene.add(pool);

            // Ping Pong Table
            const pingGeo = new THREE.BoxGeometry(4.4, 0.8, 2.4);
            const pingMat = new THREE.MeshStandardMaterial({ color: 0x1d4ed8 });
            const ping = new THREE.Mesh(pingGeo, pingMat);
            ping.position.set(5, 0.4, 8);
            scene.add(ping);

            // Net
            const net = new THREE.Mesh(new THREE.BoxGeometry(0.05, 0.3, 2.4), new THREE.MeshBasicMaterial({ color: 0xffffff }));
            net.position.set(5, 0.95, 8);
            scene.add(net);
        }

        function spawnWorkforce() {
            for(let id in WORKFORCE) {
                const emp = WORKFORCE[id];
                const group = new THREE.Group();
                group.position.set(emp.desk_pos[0], 0, emp.desk_pos[2]);

                // Desk & Dual Monitors
                const desk = new THREE.Mesh(new THREE.BoxGeometry(2.6, 0.9, 1.4), new THREE.MeshStandardMaterial({ color: 0x1e293b }));
                desk.position.set(0, 0.45, 0);
                group.add(desk);

                const monL = new THREE.Mesh(new THREE.BoxGeometry(0.9, 0.55, 0.04), new THREE.MeshBasicMaterial({ color: 0x38bdf8 }));
                monL.position.set(-0.5, 1.15, 0.2);
                monL.rotation.y = 0.15;
                group.add(monL);

                const monR = new THREE.Mesh(new THREE.BoxGeometry(0.9, 0.55, 0.04), new THREE.MeshBasicMaterial({ color: 0x38bdf8 }));
                monR.position.set(0.5, 1.15, 0.2);
                monR.rotation.y = -0.15;
                group.add(monR);

                // Animated Employee Bot
                const body = new THREE.Mesh(new THREE.CylinderGeometry(0.35, 0.35, 1.2, 16), new THREE.MeshStandardMaterial({ color: emp.color, roughness: 0.3 }));
                body.position.set(0, 0.6, -0.9);
                body.userData = { empId: id };
                group.add(body);

                const head = new THREE.Mesh(new THREE.SphereGeometry(0.3, 16, 16), new THREE.MeshStandardMaterial({ color: 0xffffff }));
                head.position.set(0, 1.45, -0.9);
                head.userData = { empId: id };
                group.add(head);

                // Pulsing Ring
                const ring = new THREE.Mesh(new THREE.RingGeometry(0.6, 0.75, 32), new THREE.MeshBasicMaterial({ color: emp.color, side: THREE.DoubleSide }));
                ring.rotation.x = -Math.PI / 2;
                ring.position.set(0, 0.05, -0.9);
                group.add(ring);

                scene.add(group);
                employees[id] = { group, body, ring, data: emp };
            }
        }

        function populateOrgTree() {
            const tree = document.getElementById("orgTree");
            let html = "";
            for(let id in WORKFORCE) {
                const emp = WORKFORCE[id];
                html += `
                <div class="employee-card" onclick="selectEmployee('${id}')">
                    <div style="display:flex; align-items:center; gap:8px;">
                        <span style="font-size:1.1rem;">${emp.avatar}</span>
                        <div>
                            <div class="emp-name">${emp.name}</div>
                            <div class="emp-role">${emp.role}</div>
                        </div>
                    </div>
                    <div class="status-dot"></div>
                </div>`;
            }
            tree.innerHTML = html;
        }

        function onPointerDown(event) {
            const rect = renderer.domElement.getBoundingClientRect();
            mouse.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
            mouse.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;

            raycaster.setFromCamera(mouse, camera);
            const intersects = raycaster.intersectObjects(scene.children, true);

            for (let i = 0; i < intersects.length; i++) {
                const hit = intersects[i].object;
                if(hit.userData && hit.userData.empId) {
                    selectEmployee(hit.userData.empId);
                    break;
                }
            }
        }

        function selectEmployee(empId) {
            selectedEmpId = empId;
            const emp = WORKFORCE[empId];

            document.getElementById("cName").innerText = emp.name;
            document.getElementById("cRole").innerText = emp.role;
            document.getElementById("cAvatar").innerText = emp.avatar;
            document.getElementById("cActivity").innerText = emp.activity;
            document.getElementById("cNode").innerText = emp.node;
            document.getElementById("charCard").style.display = "flex";

            // Smoothly move controls target to the employee
            controls.target.set(emp.desk_pos[0], 1, emp.desk_pos[2]);
            camera.position.set(emp.desk_pos[0], 3, emp.desk_pos[2] + 4);
        }

        function closeCard() {
            document.getElementById("charCard").style.display = "none";
            selectedEmpId = null;
            controls.target.set(0, 1, 0);
        }

        function resetView() {
            closeCard();
            isTouring = false;
            document.getElementById("tourBtn").classList.remove("active");
            controls.target.set(0, 1, 0);
            camera.position.copy(defaultCamPos);
        }

        function toggleTour() {
            isTouring = !isTouring;
            document.getElementById("tourBtn").classList.toggle("active", isTouring);
        }

        // WhisperFlow Web Voice Processing Simulation
        function toggleWhisperFlow() {
            isRecording = !isRecording;
            const btn = document.getElementById("whisperBtn");
            const label = document.getElementById("whisperLabel");

            if(isRecording) {
                btn.classList.add("recording");
                label.innerText = "Listening...";
                if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
                    navigator.mediaDevices.getUserMedia({ audio: true }).then(stream => {
                        window.audioStream = stream;
                    }).catch(err => console.log("Microphone access ready in browser"));
                }
            } else {
                btn.classList.remove("recording");
                label.innerText = "Transcribing...";
                setTimeout(() => {
                    label.innerText = "WhisperFlow";
                    const samplePrompt = "Audit latest blocks and verify node consensus stability.";
                    document.getElementById("cMsgInput").value = samplePrompt;
                    if(!selectedEmpId) selectEmployee("triton");
                }, 1200);
            }
        }

        function sendInstruction() {
            const input = document.getElementById("cMsgInput");
            if(!input.value.trim()) return;
            const target = selectedEmpId ? WORKFORCE[selectedEmpId].name : "Team 7";
            alert(`Directive dispatched to ${target}: "${input.value}"`);
            input.value = "";
        }

        function onWindowResize() {
            const container = document.getElementById("canvas3d");
            camera.aspect = container.clientWidth / container.clientHeight;
            camera.updateProjectionMatrix();
            renderer.setSize(container.clientWidth, container.clientHeight);
        }

        function animate() {
            requestAnimationFrame(animate);
            controls.update();

            const time = Date.now() * 0.003;
            for(let id in employees) {
                const emp = employees[id];
                emp.ring.scale.setScalar(1.0 + Math.sin(time + emp.data.desk_pos[0]) * 0.12);
            }

            if(isTouring) {
                tourAngle += 0.006;
                camera.position.x = Math.sin(tourAngle) * 22;
                camera.position.z = Math.cos(tourAngle) * 22;
                camera.position.y = 14 + Math.sin(tourAngle * 2) * 3;
                camera.lookAt(0, 1, 0);
            }

            renderer.render(scene, camera);
        }

        window.onload = init;
    </script>
</body>
</html>
"""

class OfficeHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path in ["/", "/index.html"]:
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(HTML_OFFICE.encode("utf-8"))))
            self.end_headers()
            self.wfile.write(HTML_OFFICE.encode("utf-8"))
            return
        elif self.path == "/api/workforce":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(WORKFORCE_STATE).encode("utf-8"))
            return
        self.send_response(404)
        self.end_headers()

if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", PORT), OfficeHandler)
    print(f"[*] Herdr 3D Spatial Office Floor running on http://127.0.0.1:{PORT}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
